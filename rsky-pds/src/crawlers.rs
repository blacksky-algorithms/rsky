use crate::APP_USER_AGENT;
use anyhow::Result;
use futures::stream::{self, StreamExt};
use rsky_common::time::MINUTE;
use std::time::SystemTime;

const NOTIFY_THRESHOLD: i32 = 20 * MINUTE; // 20 minutes;

#[derive(Debug, Clone)]
pub struct Crawlers {
    pub hostname: String,
    pub crawlers: Vec<String>,
    pub last_notified: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CrawlerRequest {
    pub hostname: String,
}

impl Crawlers {
    pub fn new(hostname: String, crawlers: Vec<String>) -> Self {
        Crawlers {
            hostname,
            crawlers,
            last_notified: 0,
        }
    }

    // requestCrawl must advertise this PDS's hostname, not the crawler's
    fn crawl_request(&self) -> CrawlerRequest {
        CrawlerRequest {
            hostname: self.hostname.clone(),
        }
    }

    pub async fn notify_of_update(&mut self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in millis since UNIX epoch")
            .as_millis() as usize;
        if now - self.last_notified < NOTIFY_THRESHOLD as usize {
            return Ok(());
        }
        let record = self.crawl_request();
        let _ = stream::iter(self.crawlers.clone())
            .then(|service: String| {
                let record = record.clone();
                async move {
                    let client = reqwest::Client::builder()
                        .user_agent(APP_USER_AGENT)
                        .build()?;
                    Ok::<reqwest::Response, anyhow::Error>(
                        client
                            .post(format!("{}/xrpc/com.atproto.sync.requestCrawl", service))
                            .json(&record)
                            .send()
                            .await?,
                    )
                }
            })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        self.last_notified = now;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crawl_request_advertises_pds_hostname() {
        let crawlers = Crawlers::new(
            "pds.example.com".to_string(),
            vec![
                "https://relay1.example".to_string(),
                "https://relay2.example".to_string(),
            ],
        );
        assert_eq!(crawlers.crawl_request().hostname, "pds.example.com");
    }
}
