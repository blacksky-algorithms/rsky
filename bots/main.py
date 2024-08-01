import json, requests, re, html, os, datetime, time
from PIL import Image as PILImage
from io import BytesIO
from atproto import Client, models
from dotenv import load_dotenv

load_dotenv()

BASE_URL = os.getenv('TWITTER_BASE_URL')
size = 320, 320
cached_ids = set()


def get_recent_tweets():
    start_time = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(
        minutes=int(os.getenv('TWITTER_SEARCH_INTERVAL')))
    params = {
        'start_time': start_time.isoformat(),
        'query': 'from:{0} -is:quote -is:retweet -is:reply'.format(os.getenv('TWITTER_SEARCH_FROM_ACCOUNT')),
        'max_results': int(os.getenv('TWITTER_SEARCH_MAX_RESULTS'))
    }
    headers = {
        'Authorization': 'Bearer {0}'.format(os.getenv('TWITTER_BEARER'))
    }
    url = '{0}/tweets/search/recent'.format(BASE_URL)
    try:
        r = requests.get(url, params=params, headers=headers)
        status_code = r.status_code
        while status_code == 429:
            print('Sleeping for 5 minutes before retrying searching for tweet')
            time.sleep(300)
            r = requests.get(url, params=params, headers=headers)
            status_code = r.status_code

        results = r.json()
        return results
    except Exception as e:
        print('Error getting tweet for {0}: {1}'.format(url, e))
        return None


def get_tweet(tweet_id):
    params = {
        'tweet.fields': ','.join(
            ['attachments', 'author_id', 'created_at', 'id', 'in_reply_to_user_id', 'possibly_sensitive',
             'public_metrics', 'referenced_tweets', 'source', 'text', 'withheld', 'entities']),
        'expansions': ','.join(['author_id', 'attachments.media_keys']),
        'media.fields': ','.join(['duration_ms', 'height', 'media_key', 'preview_image_url', 'type', 'url', 'width'])
    }
    headers = {
        'Authorization': 'Bearer {0}'.format(os.getenv('TWITTER_BEARER'))
    }
    url = '{0}/tweets/{1}'.format(BASE_URL, tweet_id)  # 1800227859774206420

    try:
        r = requests.get(url, params=params, headers=headers)
        status_code = r.status_code
        while status_code == 429:
            print('Sleeping for 5 minutes before retrying getting tweet {0}'.format(tweet_id))
            time.sleep(300)
            r = requests.get(url, params=params, headers=headers)
            status_code = r.status_code

        results = r.json()
        return results
    except Exception as e:
        print('Error getting tweet for {0}: {1}'.format(url, e))
        return None


def get_images(urls):
    images = []
    for url in urls:
        with requests.get(url, stream=True) as r:
            r.raise_for_status()
            image = PILImage.open(BytesIO(r.content)).convert('RGB')
            image.thumbnail(size, PILImage.Resampling.LANCZOS)
            buf = BytesIO()
            image.save(buf, format='PNG')
            images.append((buf, len(r.content)))
    return images

def get_image(url):
    image_tuple = None
    with requests.get(url, stream=True) as r:
        r.raise_for_status()
        image = PILImage.open(BytesIO(r.content)).convert('RGB')
        image.thumbnail(size, PILImage.Resampling.LANCZOS)
        buf = BytesIO()
        image.save(buf, format='PNG')
        image_tuple = (buf, len(r.content))
    return image_tuple

def parse_images(t):
    includes = t.get('includes')
    if includes:
        parsed_urls = [m.get('url') for m in includes.get('media',[]) if
                       m.get('type') == 'photo']
        #video_thumbnails = [m.get('preview_image_url') for m in includes.get('media',[]) if
        #                    m.get('type') == 'video']
        #parsed_urls.extend(video_thumbnails)
        return parsed_urls
    else:
        return []

def get_first(iterable, default=None):
    if iterable:
        for item in iterable:
            return item
    return default

def parse_embed_url(t, client):
    entities = t['data'].get('entities')
    embed = None
    if entities:
        parsed_urls = [e for e in entities.get('urls',[])]
        first_url = get_first(parsed_urls)
        if first_url and first_url.get('unwound_url') and first_url.get('media_key') is None:
            first_image = get_first(first_url.get('images'))
            blob = None
            if first_image:
                image_bytes = get_image(first_image['url'])[0].getvalue()
                blob = client.upload_blob(image_bytes).blob

            external = models.AppBskyEmbedExternal.External(
                description=first_url.get('description',' '),
                title=first_url['title'],
                uri=first_url['unwound_url'],
                thumb=blob
            )
            embed = models.AppBskyEmbedExternal.Main(external=external)

    return embed

def remove_twitter_link(txt):
    return re.sub(r'https?://t\.co\S+', '', txt)


if __name__ == '__main__':
    while True:
        try:
            client = Client()
            client.login(os.getenv('ATPROTO_USERNAME'), os.getenv('ATPROTO_PASSWORD'))

            tweets = get_recent_tweets()
            for tweet in tweets.get('data', []):
                tweet_id = tweet['id']
                edit_history = tweet['edit_history_tweet_ids']
                if tweet_id not in cached_ids:
                    try:
                        tweet = get_tweet(tweet_id)
                        image_urls = parse_images(tweet)
                        list_of_images = get_images(image_urls)
                        external_url = parse_embed_url(tweet, client)

                        if external_url:
                            client.send_post(
                                text=html.unescape(remove_twitter_link(tweet['data'].get('text'))),
                                embed=external_url
                            )
                            print('Made external url post for {}'.format(json.dumps(tweet, indent=4)))
                        elif list_of_images:
                            image_bytes = [i[0].getvalue() for i in list_of_images]
                            client.send_images(
                                text=html.unescape(remove_twitter_link(tweet['data'].get('text'))),
                                images=image_bytes,
                                image_alts=[]
                            )
                            print('Made image post for {}'.format(json.dumps(tweet, indent=4)))
                        else:
                            client.send_post(text=html.unescape(remove_twitter_link(tweet['data'].get('text'))))
                            print('Made plain text post for {}'.format(json.dumps(tweet, indent=4)))

                        cached_ids.add(tweet_id)
                        for edit in edit_history:
                            cached_ids.add(edit)
                    except Exception as e:
                        print('Error: {0} for tweet: {1}'.format(e, json.dumps(tweet, indent=4)))
        except Exception as e:
            print('Issue getting started: {0}'.format(e))
        time.sleep(int(os.getenv('TWITTER_SEARCH_INTERVAL')) * 60)
