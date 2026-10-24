//! decktrace 的领域层：牌面/窗口模型、游戏规则与记牌状态机。
//!
//! 这一层不依赖 macOS、采集后端、具体游戏或 UI。

pub mod bottom;
pub mod config;
pub mod deck;
pub mod game;
pub mod models;
pub mod recognition;

pub use bottom::{BottomConfig, BottomSpec};
pub use config::{JokerBand, RecognitionConfig, ResetConfig};
pub use deck::{CardTracker, TrackerSnapshot};
pub use game::GameProfile;
pub use models::{BBox, Detection, RawFrame, SUIT_SYMBOLS, WindowInfo, WindowSelector};
pub use recognition::{RecognitionResult, Recognizer, SharedRecognizer};
