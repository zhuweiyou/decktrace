//! 游戏接入接口：每个游戏实现自己的 `GameProfile`，通用层只依赖这里。

pub mod bottom;
pub mod profile;

pub use bottom::{BottomConfig, BottomSpec};
pub use profile::GameProfile;
