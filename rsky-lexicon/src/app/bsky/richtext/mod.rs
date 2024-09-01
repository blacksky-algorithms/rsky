#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Facet {
    pub index: ByteSlice,
    pub features: Vec<Features>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum Features {
    #[serde(rename = "app.bsky.richtext.facet#mention")]
    Mention(Mention),
    #[serde(rename = "app.bsky.richtext.facet#link")]
    Link(Link),
    #[serde(rename = "app.bsky.richtext.facet#tag")]
    Tag(Tag),
}

/// Facet feature for mention of another account. The text is usually a handle, including a '@'
/// prefix, but the facet reference is a DID.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Mention {
    pub did: String,
}

/// Facet feature for a URL. The text URL may have been simplified or truncated, but the facet
/// reference should be a complete URL.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Link {
    pub uri: String,
}

/// Facet feature for a hashtag. The text usually includes a '#' prefix, but the facet reference
/// should not (except in the case of 'double hashtags').
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Tag {
    pub tag: String,
}

/// Specifies the sub-string range a facet feature applies to.
/// Start index is inclusive, end index is exclusive.
/// Indices are zero-indexed, counting bytes of the UTF-8 encoded text.
/// NOTE: some languages, like Javascript, use UTF-16 or Unicode codepoints for string slice indexing;
/// in these languages, convert to byte arrays before working with facets.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ByteSlice {
    #[serde(rename = "byteStart")]
    pub byte_start: usize,
    #[serde(rename = "byteEnd")]
    pub byte_end: usize,
}
