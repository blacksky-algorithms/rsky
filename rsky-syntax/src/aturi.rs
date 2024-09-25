use anyhow::{bail, Result};
use lazy_static::lazy_static;
use regex::Regex;
use url::Url;

pub fn atp_uri_regex(input: &str) -> Option<Vec<&str>> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"(?i)^(at://)?((?:did:[a-z0-9:%-]+)|(?:[a-z0-9][a-z0-9.:-]*))(/[^?#\s]*)?(\?[^#\s]+)?(#[^\s]+)?$").unwrap();
    }
    if let Some(captures) = RE.captures(input) {
        Some(
            captures
                .iter()
                .skip(1) // Skip the first capture which is the entire match
                .map(|c| c.map_or("", |m| m.as_str()))
                .collect(),
        )
    } else {
        None
    }
}

pub fn relative_regex(input: &str) -> Option<Vec<&str>> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"(?i)^(/[^?#\s]*)?(\?[^#\s]+)?(#[^\s]+)?$").unwrap();
    }
    if let Some(captures) = RE.captures(input) {
        Some(
            captures
                .iter()
                .skip(1) // Skip the first capture which is the entire match
                .map(|c| c.map_or("", |m| m.as_str()))
                .collect(),
        )
    } else {
        None
    }
}

pub struct ParsedOutput {
    pub hash: String,
    pub host: String,
    pub pathname: String,
    pub search_params: Vec<(String, String)>,
}

pub struct ParsedRelativeOutput {
    pub hash: String,
    pub pathname: String,
    pub search_params: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AtUri {
    pub hash: String,
    pub host: String,
    pub pathname: String,
    pub search_params: Vec<(String, String)>,
}

impl AtUri {
    pub fn new(uri: String, base: Option<String>) -> Result<Self> {
        let parsed: ParsedOutput = match base {
            Some(base) => match parse(&base)? {
                None => bail!("Invalid at uri: `{base}`"),
                Some(parsed_base) => match parse_relative(&uri)? {
                    None => bail!("Invalid path: `{uri}`"),
                    Some(relativep) => ParsedOutput {
                        hash: relativep.hash,
                        host: parsed_base.host,
                        pathname: relativep.pathname,
                        search_params: relativep.search_params,
                    },
                },
            },
            None => match parse(&uri)? {
                None => bail!("Invalid at uri: `{uri}`"),
                Some(result) => result,
            },
        };
        Ok(Self {
            hash: parsed.hash,
            host: parsed.host,
            pathname: parsed.pathname,
            search_params: parsed.search_params,
        })
    }

    pub fn make(
        handle_or_did: String,
        collection: Option<String>,
        rkey: Option<String>,
    ) -> Result<Self> {
        let mut str = handle_or_did;
        if let Some(collection) = collection {
            str += format!("/{collection}").as_str();
        }
        if let Some(rkey) = rkey {
            str += format!("/{rkey}").as_str();
        }
        AtUri::new(str, None)
    }

    pub fn get_protocol(&self) -> String {
        "at:".to_string()
    }

    pub fn get_origin(&self) -> String {
        format!("at://{}", self.host)
    }

    pub fn get_hostname(&self) -> &String {
        &self.host
    }

    pub fn set_hostname(&mut self, v: String) -> () {
        self.host = v;
    }

    pub fn get_search(&self) -> Result<Option<String>> {
        let url = Url::parse_with_params("http://example.com", &self.search_params)?;
        match url.query() {
            Some(query) => Ok(Some(query.to_string())),
            None => Ok(None),
        }
    }

    pub fn set_search(&mut self, v: String) -> Result<()> {
        let dummy_url = format!("http://example.com{}", v);
        let url = Url::parse(&dummy_url)?;
        let query_pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|pair| (pair.0.to_string(), pair.1.to_string()))
            .collect();
        self.search_params = query_pairs;
        Ok(())
    }

    pub fn get_collection(&self) -> String {
        match &self.pathname.split("/").collect::<Vec<&str>>().get(1) {
            Some(collection) => collection.to_string(),
            None => "".to_string(),
        }
    }

    pub fn set_collection(&mut self, v: String) -> () {
        let mut parts: Vec<String> = self
            .pathname
            .split("/")
            .collect::<Vec<&str>>()
            .into_iter()
            .map(|p| p.to_string())
            .collect::<Vec<String>>();
        if parts.len() > 0 {
            parts[0] = v;
        } else {
            parts.push(v);
        }
        self.pathname = parts.join("/");
    }

    pub fn get_rkey(&self) -> String {
        match &self.pathname.split("/").collect::<Vec<&str>>().get(2) {
            Some(rkey) => rkey.to_string(),
            None => "".to_string(),
        }
    }

    pub fn set_rkey(&mut self, v: String) -> () {
        let mut parts: Vec<String> = self
            .pathname
            .split("/")
            .collect::<Vec<&str>>()
            .into_iter()
            .map(|p| p.to_string())
            .collect::<Vec<String>>();
        if parts.len() > 1 {
            parts[1] = v;
        } else if parts.len() > 0 {
            parts.push(v);
        } else {
            parts.push("undefined".to_string());
            parts.push(v);
        }
        self.pathname = parts.join("/");
    }

    pub fn get_href(&self) -> String {
        self.to_string()
    }

    pub fn to_string(&self) -> String {
        let mut path = match self.pathname == "" {
            true => "/".to_string(),
            false => self.pathname.clone(),
        };
        if !path.starts_with("/") {
            path = format!("/{path}");
        }
        let qs = match self.get_search() {
            Ok(Some(search_params)) if !search_params.starts_with("?") && search_params != "" => {
                format!("?{search_params}")
            }
            Ok(Some(search_params)) => search_params,
            _ => "".to_string(),
        };
        let hash = match self.hash == "" {
            true => self.hash.clone(),
            false => format!("#{}", self.hash),
        };
        format!("at://{}{}{}{}", self.host, path, qs, hash)
    }
}

pub fn parse(str: &String) -> Result<Option<ParsedOutput>> {
    match atp_uri_regex(str) {
        None => Ok(None),
        Some(matches) => {
            // The query string we want to parse
            // e.g. `?q=URLUtils.searchParams&topic=api`
            let query_string = matches[3];
            // Create a dummy base URL and append the query string
            let dummy_url = format!("http://example.com{}", query_string);
            // Parse the URL
            let url = Url::parse(&dummy_url)?;
            let query_pairs: Vec<(String, String)> = url
                .query_pairs()
                .map(|pair| (pair.0.to_string(), pair.1.to_string()))
                .collect();
            Ok(Some(ParsedOutput {
                hash: matches[4].to_string(),
                host: matches[1].to_string(),
                pathname: matches[2].to_string(),
                search_params: query_pairs,
            }))
        }
    }
}

pub fn parse_relative(str: &String) -> Result<Option<ParsedRelativeOutput>> {
    match relative_regex(str) {
        None => Ok(None),
        Some(matches) => {
            // The query string we want to parse
            // e.g. `?q=URLUtils.searchParams&topic=api`
            let query_string = matches[1];
            // Create a dummy base URL and append the query string
            let dummy_url = format!("http://example.com{}", query_string);
            // Parse the URL
            let url = Url::parse(&dummy_url)?;
            let query_pairs: Vec<(String, String)> = url
                .query_pairs()
                .map(|pair| (pair.0.to_string(), pair.1.to_string()))
                .collect();
            Ok(Some(ParsedRelativeOutput {
                hash: matches[2].to_string(),
                pathname: matches[0].to_string(),
                search_params: query_pairs,
            }))
        }
    }
}
