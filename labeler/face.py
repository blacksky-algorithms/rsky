import requests, os, json, argparse
from PIL import Image as PILImage
from io import BytesIO
from peewee import *
from playhouse.postgres_ext import *
from datetime import datetime, timedelta
from dotenv import load_dotenv
from tqdm import tqdm
from deepface import DeepFace
from atproto import Client
import numpy as np

load_dotenv()

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

class Membership(BaseModel):
	did = CharField(primary_key=True, null=False)
	included = BooleanField()
	excluded = BooleanField()
	LIST = CharField(null=False)

def get_pfp(did, client):
	profile = client.app.bsky.actor.get_profile({'actor': did})
	with requests.get(profile.avatar, stream=True) as r:
		r.raise_for_status()
		
		image = PILImage.open(BytesIO(r.content)).convert('RGB')

		return (image, len(r.content), profile.display_name)

if __name__ == '__main__':
	parser = argparse.ArgumentParser(
		description = 'A simple script for leveraging ML models to classify Bsky pfps.')

	parser.add_argument(
		'-d',
		'--did', 
		type=str, 
		required=True, 
		help='<Required> DID of actor')

	parser.add_argument(
		'-a',
		'--actions', 
		action='append', 
		required=True,
		help='<Required> Set actions to analyze against')

	args = parser.parse_args()

	client = Client()
	client.login(os.getenv('ATPROTO_USERNAME'), os.getenv('ATPROTO_PASSWORD'))

	image, fsize, name = get_pfp(args.did, client)
	try:
		print(f'Analyzing face for {name}')
		image.show()
		objs = DeepFace.analyze(
			img_path = np.asarray(image), 
			actions = args.actions, # ['race', 'age', 'gender', 'emotion']
			enforce_detection = False
		)
		print(json.dumps(objs, indent=4))
	except Exception as e:
		# This can happen for pfp without a face
		print("Error identifying face in profile {0}: {1}".format(args.did, e))
