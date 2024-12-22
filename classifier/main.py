import bottle, torch, clip, requests, os, json, argparse
from bottle import auth_basic, route, request, run
from PIL import Image as PILImage
from io import BytesIO
from torchvision.transforms import Compose, Resize, CenterCrop, ToTensor, Normalize
from clip import load
from peewee import *
from playhouse.postgres_ext import *
from datetime import datetime, timedelta
from dotenv import load_dotenv
from tqdm import tqdm
import numpy as np
from torchvision import models
from torch.autograd import Variable
from torch import nn
from numpy import array

load_dotenv()

# Image transformation pipeline
transform = Compose([
	Resize(256),
	CenterCrop(224),
	ToTensor(),
	Normalize(mean=[0.485, 0.456, 0.406],
			std=[0.229, 0.224, 0.225])
])

# Support for Array fields
database = PostgresqlExtDatabase(
	os.getenv('DATABASE_NAME'), 
	dsn=os.getenv('DATABASE_URL'))

# model definitions -- the standard "pattern" is to define a base model class
# that specifies which database to use.  then, any subclasses will automatically
# use the correct storage.
class BaseModel(Model):
	class Meta:
		database = database

class Image(BaseModel):
	cid = CharField(primary_key=True, null=False)
	alt = CharField()
	postCid = CharField(null=False)
	postUri = CharField(null=False)
	createdAt = CharField(null=False)
	indexedAt = CharField(null=False)
	labels = ArrayField(TextField)

def get_blob(did, cid):
	url = 'https://bsky.social/xrpc/com.atproto.sync.getBlob'
	params = {
		'did': did,
		'cid': cid
	}

	with requests.get(url, params=params, stream=True) as r:
		r.raise_for_status()
		
		image = PILImage.open(BytesIO(r.content)).convert('RGB')

		return (image, len(r.content))

def openai_classify(image, size, categories):		
	# Preprocess the image
	image = transform(image)
	image = image.unsqueeze(0).to(device)
	
	# Prepare the text inputs
	text = torch.cat([clip.tokenize(f"a {c} image") for c in categories]).to(device)
	
	# Compute the features and compare the image to the text inputs
	with torch.no_grad():
		image_features = modelA.encode_image(image)
		text_features = modelA.encode_text(text)
		
	# Compute the raw similarity score
	similarity = (image_features @ text_features.T)
	similarity_softmax = similarity.softmax(dim=-1)
	
	# Define a threshold
	threshold = 10.0

	# Get the highest scoring category
	max_raw_score = torch.max(similarity)
	if max_raw_score < threshold:
		return {
			"file_size": size, 
			"category": "none", 
			"similarity_score": 0,
			"values": [0.0 for _ in categories]
		}
	else:
		category_index = similarity_softmax[0].argmax().item()
		category = categories[category_index]
		similarity_score = similarity_softmax[0, category_index].item()
		values = similarity[0].tolist()
		return {
			"file_size": size, 
			"category": category, 
			"similarity_score": similarity_score,
			"values": values
		}

def nn_classify(image):		
	image_tensor = transform(image).float()
	image_tensor = image_tensor.unsqueeze_(0)

	if torch.cuda.is_available():
		image_tensor.cuda()

	input = Variable(image_tensor)
	output = modelB(input)
	index = output.data.numpy().argmax()
	return index

if __name__ == '__main__':
	parser = argparse.ArgumentParser(
		description = 'A simple script for leveraging ML models to label AT Protocol images as NSFW.')
	parser.add_argument(
		'-m',
		'--model', 
		type=str, 
		required=True, 
		help='<Required> ML model to use for labeling')
	parser.add_argument(
		'-H',
		'--hours', 
		type=int, 
		required=True, 
		help='<Required> Number of hours ago')
	parser.add_argument(
		'-l',
		'--labels', 
		action='append', 
		required=True,
		help='<Required> Set labels')
	parser.add_argument(
		'-A',
		'--thresholdA', 
		type=int, 
		default=3, 
		help='Index threshold to use (0-5)')
	parser.add_argument(
		'-B',
		'--thresholdB', 
		type=int, 
		default=80, 
		help='Index threshold to use (0-100)')

	args = parser.parse_args()

	local_config = {'users':[]}

	try:
		with open('labeler/config.json', 'r') as config:
			loaded_config = json.load(config)
			local_config['users'] = loaded_config.get('users', [])
			print('found config: ', json.dumps(local_config, indent=4))
	except OSError as e:
		print('No config found. No filters will be applied.')

	print('Loading model A..')
	# Load the CLIP model
	device = "cuda" if torch.cuda.is_available() else "cpu"
	modelA, preprocess = load(args.model, device=device)

	print('Loading model B..')
	# Load the local model
	modelB = models.resnet50()
	modelB.fc = nn.Sequential(nn.Linear(2048, 512),
										nn.ReLU(),
										nn.Dropout(0.2),
										nn.Linear(512, 10),
										nn.LogSoftmax(dim=1))
	modelB.load_state_dict(torch.load('labeler/ResNet50_nsfw_model.pth', map_location=torch.device('cpu')))
	modelB.eval()

	print('Connecting to database..')
	database.connect(reuse_if_open=True)
	interval = datetime.utcnow() - timedelta(hours=args.hours, minutes=0)

	print('Querying images..')
	query = Image.select().where((Image.indexedAt > interval.isoformat()) & (Image.labels.is_null(True) | ~Image.labels.contains_any('sexy','nsfw-fp')))
	

	for img in tqdm(query):
		did = img.postUri[5:37]
		rkey = img.postUri.split('/')[-1]
		if not local_config['users'] or did not in local_config['users']:
			try:
				image, fsize = get_blob(did, img.cid)
				res = openai_classify(image, fsize, args.labels)
				index = nn_classify(image)
				values = array(res['values'])
				if index >= args.thresholdA and all(values > args.thresholdB):
					print(f'Image checked is: {img.cid}')
					print(f'INDEX {index} FOR https://bsky.app/profile/{did}/post/{rkey}')
					print(json.dumps(res, indent=4))
					if not img.labels:
						# Initialize label array
						img.labels = ['sexy']
						img.save()
					elif 'sexy' not in img.labels: 
						# If there are existing labels, append
						img.labels.append('sexy')
						img.save()
					else:
						pass
			except Exception as e:
				# This can happen for deleted posts where the image wasn't deleted in the db
				print("Error classifying image in post {0}: {1}".format(img.postUri, e))
		else:
			pass

	if not database.is_closed():
		database.close()
	print('Finished.')
