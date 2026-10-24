//! 通用模板识别引擎：点数 + 花色成对校验，底牌识别交给游戏 profile。

mod engine;

pub use engine::{CardRecognizer, RecognitionError, RecognitionResult, annotate};
