//! 欢乐斗地主：游戏配置、牌面素材与模板识别实现。

pub mod profile;
pub mod recognition;

pub use profile::profile;
pub use recognition::{HlddzRecognizer, RecognitionError};
