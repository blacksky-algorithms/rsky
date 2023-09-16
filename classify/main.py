import bottle
from bottle import auth_basic, route, request, run
from PIL import Image
from io import BytesIO
import torch
from torchvision.transforms import Compose, Resize, CenterCrop, ToTensor, Normalize
import clip
from clip import load
import requests
import os

app = bottle.Bottle()
bottle.BaseRequest.MEMFILE_MAX = 1024 * 1024 # (or whatever you want)

# Environment variables
model_name = os.getenv("OPENAI_MODEL", 'ViT-L/14')
host = os.getenv("BOTTLE_HOST", '0.0.0.0')
port = int(os.getenv("BOTTLE_PORT", '8181'))

# Load the CLIP model
device = "cuda" if torch.cuda.is_available() else "cpu"
model, preprocess = load(model_name, device=device)

# Image transformation pipeline
transform = Compose([
    Resize(256),
    CenterCrop(224),
    ToTensor(),
    Normalize(mean=[0.485, 0.456, 0.406],
            std=[0.229, 0.224, 0.225])
])

def handle_404(error):
    return "404 Error Not Found"

def handle_401(error):
    return "401 Error Not Authorized"

def handle_500(error):
    return "500 Error Internal"

def check(user, pw):
    if user == os.getenv("BOTTLE_USER", 'admin') and pw == os.getenv("BOTTLE_PW", 'admin'): return True

@app.route('/classify_image/', method='GET')
@auth_basic(check)
def classify_image():
    # Get categories from query parameters
    categories = request.query.getall('category')
    url = 'https://bsky.social/xrpc/com.atproto.sync.getBlob'
    params = {
        'did': request.query.get('did'),
        'cid': request.query.get('cid')
    }

    with requests.get(url, params=params, stream=True) as r:
        r.raise_for_status()

        image = Image.open(BytesIO(r.content)).convert('RGB')
        
        # Preprocess the image
        image = transform(image)
        image = image.unsqueeze(0).to(device)
        
        print("categories: ", categories)

        # Prepare the text inputs
        text = torch.cat([clip.tokenize(f"a {c} image") for c in categories]).to(device)
        
        # Compute the features and compare the image to the text inputs
        with torch.no_grad():
            image_features = model.encode_image(image)
            text_features = model.encode_text(text)
            
        # Compute the raw similarity score
        similarity = (image_features @ text_features.T)
        similarity_softmax = similarity.softmax(dim=-1)
        
        # Define a threshold
        threshold = 10.0

        # Get the highest scoring category
        max_raw_score = torch.max(similarity)
        if max_raw_score < threshold:
            return {
                "file_size": len(r.content), 
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
                "file_size": len(r.content), 
                "category": category, 
                "similarity_score": similarity_score,
                "values": values
            }

app.error_handler = {
    404: handle_404,
    401: handle_401,
    500: handle_500
}

run(app=app, host=host, port=port)
