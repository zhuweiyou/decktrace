//! 王者斗地主：**基础架子**。
//!
//! 目前只有 `GameProfile` 与一个占位识别器（不识别任何牌，返回空结果）。
//! 识别实现与模板/素材待后续补齐；补齐后把 `WzddzRecognizer::recognize`
//! 换成真实实现即可，接口不变。

pub mod profile;
pub mod recognition;

pub use profile::{profile, profile_with_assets};
pub use recognition::{RecognitionError, WzddzRecognizer};
