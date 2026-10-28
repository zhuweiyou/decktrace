//! 欢乐斗地主（手机横屏小程序）：游戏配置、牌面素材与模板识别实现。
//!
//! 与 PC 版 `hlddz` 相互独立：模板/区域/hand 比例按手机端单独标定，
//! 修改这里不会影响 PC 版。

pub mod profile;
pub mod recognition;

pub use profile::profile;
pub use recognition::{HlddzRecognizer, RecognitionError};
