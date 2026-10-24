//! decktrace 的领域层：牌面/窗口模型、游戏规则与记牌状态机。
//!
//! 这一层不依赖 macOS、采集后端、具体游戏或 UI。

pub mod config;
pub mod deck;
pub mod models;
pub mod rules;

pub use config::{JokerBand, RecognitionConfig, ResetConfig};
pub use deck::{CardTracker, TrackerSnapshot};
pub use models::{BBox, Detection, RawFrame, SUIT_SYMBOLS, WindowInfo, WindowSelector};
pub use rules::DeckRules;
