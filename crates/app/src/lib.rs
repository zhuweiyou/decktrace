//! decktrace GUI 应用库：会话运行时与游戏注册表。

pub mod registry;
pub mod session;

pub use registry::{GAME_IDS, create_recognizer, get_game, is_mobile_game, list_games};
pub use session::{Session, SessionConfig};
