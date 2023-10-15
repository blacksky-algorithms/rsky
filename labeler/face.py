import requests, os, json, argparse, csv
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
	list = CharField(null=False)

def get_pfp(did, client):
	profile = client.app.bsky.actor.get_profile({'actor': did})
	if profile.avatar:
		with requests.get(profile.avatar, stream=True) as r:
			if r.status_code == 429:
				print('Sleeping for 5 mins. before retrying')
				time.sleep(300)
				return get_pfp(did, client)
			else:
				r.raise_for_status()
				
				image = PILImage.open(BytesIO(r.content)).convert('RGB')

				return (image, profile.avatar, profile.display_name)
	else:
		return (None, None, None)

def get_users():
	is_done = False
	url = 'https://bsky.social/xrpc/com.atproto.sync.listRepos'
	params = {}
	users = []

	while not is_done:
		r = requests.get(url, params=params).json()
		users.extend(r['repos'])
		params['cursor'] = r.get('cursor')

		if not params['cursor']:
			is_done = True

	return users

if __name__ == '__main__':
	parser = argparse.ArgumentParser(
		description = 'A simple script for leveraging ML models to classify Bsky pfps.')

	parser.add_argument(
		'-a',
		'--actions', 
		action='append', 
		required=True,
		help='<Required> Set actions to analyze against')

	args = parser.parse_args()

	client = Client()
	client.login(os.getenv('ATPROTO_USERNAME'), os.getenv('ATPROTO_PASSWORD'))
	print('Getting existing members...')
	members = [member.did for member in tqdm(Membership.select().where(Membership.included == True))]

	print('Done! Getting all users...')
	all_users = None
	with open('all_users.json', 'r') as openfile:
		all_users = json.load(openfile)

	if not all_users:
		all_users = get_users()
		json_object = json.dumps(all_users, indent=4)
		with open("all_users.json", "w") as outfile:
			outfile.write(json_object)

	dids = [user['did'] for user in all_users if user['did'] not in members]
	new_members = []

	for did in tqdm(dids):
		try:
			image, avatar, name = get_pfp(did, client)
			if image:
				try:
					result = DeepFace.analyze(
						img_path = np.asarray(image), 
						actions = args.actions, # ['race', 'age', 'gender', 'emotion']
						enforce_detection = True,
						silent=True
					)
					if result:
						result = result[0]
						user_record = {
							'did': did,
							'avatar': avatar,
							'display_name': name
						}
						if result['race']['black'] > 65:
							new_members.append(user_record)
					else:
						print(f"Error getting face info for {did}: List was empty")
				except Exception as e:
					# This can happen for pfp without a face
					#nprint(f"Error identifying face in profile {did}: {e}")
					pass
		except Exception as e:
			print(f"Error getting pfp in profile {did}: {e}")

	now = datetime.now().isoformat()
	with open(f'new_blacksky_users_{now}.tsv', 'w', newline='') as tsvfile:
		writer = csv.writer(tsvfile, delimiter='\t', lineterminator='\n')
		count = 0
		print('Done! Writing to CSV...')
		for new_member in tqdm(new_members):
			if not new_member: continue

			if count == 0:
		 
				# Writing headers of CSV file
				header = new_member.keys()
				writer.writerow(header)
				count += 1
		 	
			# Writing data of CSV file
			writer.writerow(new_member.values())
 	
	print('Done!')

	if not database.is_closed():
		database.close()
	print('Finished.')