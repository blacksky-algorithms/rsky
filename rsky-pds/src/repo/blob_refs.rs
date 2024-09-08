use anyhow::Result;
use lexicon_cid::Cid;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CidLinkRef {
    #[serde(rename = "$link")]
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TypedJsonBlobRef {
    #[serde(rename = "$type")]
    pub r#type: String, // `blob`
    pub r#ref: CidLinkRef,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct UntypedJsonBlobRef {
    pub cid: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum JsonBlobRef {
    Typed(TypedJsonBlobRef),
    Untyped(UntypedJsonBlobRef),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BlobRef {
    pub original: JsonBlobRef,
}

impl BlobRef {
    pub fn new(r#ref: Cid, mime_type: String, size: i64, original: Option<JsonBlobRef>) -> BlobRef {
        if let Some(o) = original {
            BlobRef { original: o }
        } else {
            let o = JsonBlobRef::Typed(TypedJsonBlobRef {
                r#type: "blob".to_owned(),
                r#ref: CidLinkRef {
                    link: r#ref.to_string(),
                },
                mime_type,
                size,
            });
            BlobRef { original: o }
        }
    }

    pub fn get_cid(&self) -> Result<Cid> {
        match &self.original {
            JsonBlobRef::Typed(typed) => Ok(Cid::from_str(&typed.r#ref.link)?),
            JsonBlobRef::Untyped(untyped) => Ok(Cid::from_str(&untyped.cid)?),
        }
    }

    pub fn get_mime_type(&self) -> &String {
        match &self.original {
            JsonBlobRef::Typed(typed) => &typed.mime_type,
            JsonBlobRef::Untyped(untyped) => &untyped.mime_type,
        }
    }

    pub fn get_size(&self) -> Option<i64> {
        match &self.original {
            JsonBlobRef::Typed(typed) => Some(typed.size),
            JsonBlobRef::Untyped(_) => None,
        }
    }

    pub fn from_json_ref(json: JsonBlobRef) -> Result<BlobRef> {
        match json {
            JsonBlobRef::Typed(j) => Ok(BlobRef::new(
                Cid::from_str(&j.r#ref.link)?,
                j.mime_type,
                j.size,
                None,
            )),
            JsonBlobRef::Untyped(ref j) => Ok(BlobRef::new(
                Cid::from_str(&j.cid)?,
                j.mime_type.clone(),
                -1,
                Some(json),
            )),
        }
    }

    pub fn ipld(&self) -> TypedJsonBlobRef {
        if let JsonBlobRef::Typed(j) = &self.original {
            TypedJsonBlobRef {
                r#type: "blob".to_owned(),
                r#ref: j.r#ref.clone(),
                mime_type: j.mime_type.clone(),
                size: j.size,
            }
        } else {
            panic!("Not a TypedJsonBlobRef")
        }
    }
}
