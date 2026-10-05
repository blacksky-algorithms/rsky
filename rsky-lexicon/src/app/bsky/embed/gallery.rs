use crate::app::bsky::embed::images::AspectRatio;
use crate::com::atproto::repo::Blob;

/// An assortment of media embedded in a Bluesky record (eg, a post).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Gallery {
    pub items: Vec<GalleryItem>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum GalleryItem {
    #[serde(rename = "app.bsky.embed.gallery#image")]
    Image(Image),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub image: Blob,
    /// Alt text description of the image, for accessibility.
    pub alt: String,
    pub aspect_ratio: AspectRatio,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.embed.gallery#view")]
pub struct View {
    pub items: Vec<ViewItem>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum ViewItem {
    #[serde(rename = "app.bsky.embed.gallery#viewImage")]
    Image(ViewImage),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewImage {
    /// Fully-qualified URL where a thumbnail of the image can be fetched.
    pub thumbnail: String,
    /// Fully-qualified URL where a large version of the image can be fetched.
    pub fullsize: String,
    /// Alt text description of the image, for accessibility.
    pub alt: String,
    pub aspect_ratio: AspectRatio,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::bsky::embed::{EmbedViews, Embeds, MediaUnion, MediaViewUnion};
    use crate::app::bsky::feed::Post;
    use serde_json::json;

    fn gallery_json() -> serde_json::Value {
        json!({
            "$type": "app.bsky.embed.gallery",
            "items": [{
                "$type": "app.bsky.embed.gallery#image",
                "image": {
                    "$type": "blob",
                    "ref": { "$link": "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4" },
                    "mimeType": "image/jpeg",
                    "size": 1234
                },
                "alt": "a photo",
                "aspectRatio": { "width": 4, "height": 3 }
            }]
        })
    }

    #[test]
    fn a_post_with_a_gallery_embed_deserializes() {
        let post: Post = serde_json::from_value(json!({
            "$type": "app.bsky.feed.post",
            "createdAt": "2026-10-04T14:00:00.000Z",
            "text": "more than four images",
            "embed": gallery_json()
        }))
        .unwrap();
        let Some(Embeds::Gallery(gallery)) = post.embed else {
            panic!("expected a gallery embed");
        };
        assert_eq!(gallery.items.len(), 1);
        let GalleryItem::Image(image) = &gallery.items[0];
        assert_eq!(image.alt, "a photo");
        assert_eq!(image.aspect_ratio.width, 4);
        assert_eq!(image.aspect_ratio.height, 3);
    }

    #[test]
    fn a_gallery_embed_round_trips_as_record_media() {
        let media: MediaUnion = serde_json::from_value(gallery_json()).unwrap();
        assert!(matches!(media, MediaUnion::Gallery(_)));
        let embed: Embeds = serde_json::from_value(gallery_json()).unwrap();
        assert_eq!(serde_json::to_value(&embed).unwrap(), gallery_json());
    }

    #[test]
    fn a_gallery_view_deserializes_in_both_view_unions() {
        let view = json!({
            "$type": "app.bsky.embed.gallery#view",
            "items": [{
                "$type": "app.bsky.embed.gallery#viewImage",
                "thumbnail": "https://cdn.example.com/thumb.jpg",
                "fullsize": "https://cdn.example.com/full.jpg",
                "alt": "a photo",
                "aspectRatio": { "width": 4, "height": 3 }
            }]
        });
        let media: MediaViewUnion = serde_json::from_value(view.clone()).unwrap();
        let MediaViewUnion::GalleryView(media) = media else {
            panic!("expected a gallery view");
        };
        let ViewItem::Image(image) = &media.items[0];
        assert_eq!(image.thumbnail, "https://cdn.example.com/thumb.jpg");
        let embed: EmbedViews = serde_json::from_value(view.clone()).unwrap();
        assert_eq!(embed, EmbedViews::GalleryView(media));
        assert_eq!(serde_json::to_value(&embed).unwrap(), view);
    }
}
