//! cseek-core: a headless Soulseek client engine.

pub mod client;
pub mod config;
pub mod db;
pub mod model;
pub mod net;

mod discovery;
mod distributed;
mod downloads;
mod geoip;
mod guard;
mod lidarr;
mod messages;
mod nowplaying;
mod peers;
mod rooms;
mod search;
mod shares;
mod transfers;
mod uploads;
mod users;
mod wishlist;

pub use client::{Client, VERSION};
pub use config::Config;
pub use downloads::{DownloadItem, SourceRef};
pub use lidarr::LidarrStatus;
pub use model::*;
pub use users::{GROUP_BLACKLISTED, GROUP_LEECHERS};
pub use wishlist::WishlistPatch;
