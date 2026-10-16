//! Soulseek native discovery: our liked/hated items, recommendations, similar users.

use crate::client::{Client, Waited};
use crate::model::*;
use anyhow::{bail, Result};
use slsk_proto::{ServerRequest, ServerResponse};
use std::time::Duration;

const INTERESTS_KEY: &str = "interests";
const TIMEOUT: Duration = Duration::from_secs(15);

fn scored(v: Vec<(String, i32)>) -> Vec<ScoredItem> {
    v.into_iter().map(|(item, score)| ScoredItem { item, score }).collect()
}

impl Client {
    pub fn interests(&self) -> Interests {
        self.db
            .kv_get(INTERESTS_KEY)
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or(Interests { likes: vec![], hates: vec![] })
    }

    fn save_interests(&self, i: &Interests) {
        self.db.kv_set(INTERESTS_KEY, &serde_json::to_string(i).unwrap_or_default());
    }

    pub fn add_interest(&self, item: &str, hate: bool) -> Result<Interests> {
        let item = item.trim().to_lowercase();
        if item.is_empty() {
            bail!("item is empty");
        }
        let mut i = self.interests();
        // an item is either liked or hated, never both
        if hate {
            if i.likes.contains(&item) {
                i.likes.retain(|x| x != &item);
                let _ = self.server_send(ServerRequest::RemoveThingILike { item: item.clone() });
            }
            if !i.hates.contains(&item) {
                i.hates.push(item.clone());
            }
            let _ = self.server_send(ServerRequest::AddThingIHate { item });
        } else {
            if i.hates.contains(&item) {
                i.hates.retain(|x| x != &item);
                let _ = self.server_send(ServerRequest::RemoveThingIHate { item: item.clone() });
            }
            if !i.likes.contains(&item) {
                i.likes.push(item.clone());
            }
            let _ = self.server_send(ServerRequest::AddThingILike { item });
        }
        self.save_interests(&i);
        Ok(i)
    }

    pub fn remove_interest(&self, item: &str, hate: bool) -> Interests {
        let item = item.trim().to_lowercase();
        let mut i = self.interests();
        if hate {
            i.hates.retain(|x| x != &item);
            let _ = self.server_send(ServerRequest::RemoveThingIHate { item });
        } else {
            i.likes.retain(|x| x != &item);
            let _ = self.server_send(ServerRequest::RemoveThingILike { item });
        }
        self.save_interests(&i);
        i
    }

    /// The server forgets interests between sessions.
    pub(crate) fn resend_interests(&self) {
        let i = self.interests();
        for item in i.likes {
            let _ = self.server_send(ServerRequest::AddThingILike { item });
        }
        for item in i.hates {
            let _ = self.server_send(ServerRequest::AddThingIHate { item });
        }
    }

    pub async fn recommendations(&self) -> Result<Recommendations> {
        match self.server_request(ServerRequest::GetRecommendations, "recommendations".into(), TIMEOUT).await? {
            ServerResponse::Recommendations { recommendations, unrecommendations } => {
                Ok(Recommendations { recommendations: scored(recommendations), unrecommendations: scored(unrecommendations) })
            }
            _ => bail!("unexpected reply"),
        }
    }

    pub async fn global_recommendations(&self) -> Result<Recommendations> {
        match self.server_request(ServerRequest::GetGlobalRecommendations, "global-recommendations".into(), TIMEOUT).await? {
            ServerResponse::GlobalRecommendations { recommendations, unrecommendations } => {
                Ok(Recommendations { recommendations: scored(recommendations), unrecommendations: scored(unrecommendations) })
            }
            _ => bail!("unexpected reply"),
        }
    }

    pub async fn similar_users(&self) -> Result<Vec<SimilarUser>> {
        match self.server_request(ServerRequest::GetSimilarUsers, "similar-users".into(), TIMEOUT).await? {
            ServerResponse::SimilarUsers { users } => {
                let mut v: Vec<SimilarUser> = users.into_iter().map(|(username, rating)| SimilarUser { username, rating }).collect();
                v.sort_by(|a, b| b.rating.cmp(&a.rating));
                Ok(v)
            }
            _ => bail!("unexpected reply"),
        }
    }

    pub async fn item_info(&self, item: &str) -> Result<ItemInfo> {
        let key = item.to_lowercase();
        let rec_rx = self.register(format!("item-rec:{key}"));
        let users_rx = self.register(format!("item-users:{key}"));
        self.server_send(ServerRequest::GetItemRecommendations { item: item.to_owned() })?;
        self.server_send(ServerRequest::GetItemSimilarUsers { item: item.to_owned() })?;
        let (rec, users) = tokio::join!(
            Self::wait(rec_rx, TIMEOUT, "item recommendations"),
            Self::wait(users_rx, TIMEOUT, "item similar users")
        );
        let recommendations = match rec {
            Ok(Waited::Server(ServerResponse::ItemRecommendations { recommendations, .. })) => scored(recommendations),
            _ => vec![],
        };
        let similar_users = match users {
            Ok(Waited::Server(ServerResponse::ItemSimilarUsers { users, .. })) => users,
            _ => vec![],
        };
        Ok(ItemInfo { recommendations, similar_users })
    }
}
