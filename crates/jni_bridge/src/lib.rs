//! Android JNI 桥：把 MediaProjection 传来的 RGBA 帧送进识别 + 记牌状态机，
//! 返回一行紧凑快照字符串供悬浮窗解析。
//!
//! 快照格式（分号分段，逗号列表）：
//! `armed:1;seen:17;remaining:37;cards:2S,2H,...;bombs:8S,8H,...;bottom:QD,JD,10H`
//! 空列表用空字符串。

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jint, jlong, jstring};
use tracking::{CardTracker, GameProfile, SharedRecognizer, TrackerSnapshot};

struct Engine {
    game: Arc<GameProfile>,
    recognizer: SharedRecognizer,
    tracker: CardTracker,
    started: Instant,
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zhuweiyou_decktrace_NativeBridge_create(
    mut env: JNIEnv,
    _class: JClass,
    assets_dir: JString,
    game_id: JString,
) -> jlong {
    let assets: String = match env.get_string(&assets_dir) {
        Ok(s) => s.into(),
        Err(err) => {
            eprintln!("[jni] get assets path failed: {err}");
            return 0;
        }
    };
    let game_id: String = match env.get_string(&game_id) {
        Ok(s) => s.into(),
        Err(err) => {
            eprintln!("[jni] get game id failed: {err}");
            return 0;
        }
    };
    let game_id = if game_id.trim().is_empty() {
        "wzddz".to_string()
    } else {
        game_id
    };

    let assets_path = PathBuf::from(&assets);
    let (game, recognizer): (GameProfile, SharedRecognizer) = match game_id.as_str() {
        "hlddz_mobile" => {
            let game = hlddz_mobile::profile_with_assets(assets_path);
            match hlddz_mobile::HlddzRecognizer::new(&game) {
                Ok(rec) => (game, Arc::new(rec) as SharedRecognizer),
                Err(err) => {
                    eprintln!("[jni] hlddz_mobile 识别器初始化失败：{err}");
                    return 0;
                }
            }
        }
        "wzddz" => {
            let game = wzddz::profile_with_assets(assets_path);
            match wzddz::WzddzRecognizer::new(&game) {
                Ok(rec) => (game, Arc::new(rec) as SharedRecognizer),
                Err(err) => {
                    eprintln!("[jni] wzddz 识别器初始化失败：{err}");
                    return 0;
                }
            }
        }
        other => {
            eprintln!("[jni] 未知游戏 id：{other}");
            return 0;
        }
    };
    if let Err(err) = game.validate() {
        eprintln!("[jni] game validate failed: {err}");
        return 0;
    }
    eprintln!("[jni] create game={} assets={}", game.id, assets);

    let game = Arc::new(game);
    let tracker = CardTracker::new(game.clone(), 2, 0.92, 1.6);
    let engine = Engine {
        game,
        recognizer,
        tracker,
        started: Instant::now(),
    };
    Box::into_raw(Box::new(Mutex::new(engine))) as jlong
}

/// 送一帧 RGBA_8888（含行填充 rowStride，单位字节），返回快照字符串。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zhuweiyou_decktrace_NativeBridge_pushFrame(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    bytes: JByteArray,
    width: jint,
    height: jint,
    row_stride: jint,
) -> jstring {
    let result = push_frame_impl(&mut env, handle, &bytes, width, height, row_stride);
    match result {
        Some(text) => match env.new_string(text) {
            Ok(s) => s.into_raw(),
            Err(err) => {
                eprintln!("[jni] new_string failed: {err}");
                std::ptr::null_mut()
            }
        },
        None => std::ptr::null_mut(),
    }
}

fn push_frame_impl(
    env: &mut JNIEnv,
    handle: jlong,
    bytes: &JByteArray,
    width: jint,
    height: jint,
    row_stride: jint,
) -> Option<String> {
    if handle == 0 || width <= 0 || height <= 0 {
        return None;
    }
    let engine_mutex = unsafe { &*(handle as *const Mutex<Engine>) };
    let mut engine = engine_mutex.lock().ok()?;

    let len = env.get_array_length(bytes).ok()? as usize;
    let mut raw = vec![0i8; len];
    env.get_byte_array_region(bytes, 0, &mut raw).ok()?;
    let data: Vec<u8> = raw.into_iter().map(|b| b as u8).collect();

    let w = width as u32;
    let h = height as u32;
    let stride = (row_stride.max(width) as usize).max(w as usize * 4);
    let mut image = image::RgbImage::new(w, h);
    for y in 0..h {
        let row = y as usize * stride;
        for x in 0..w {
            let i = row + x as usize * 4;
            if i + 2 < data.len() {
                image.put_pixel(x, y, image::Rgb([data[i], data[i + 1], data[i + 2]]));
            }
        }
    }

    let recognition = engine.recognizer.recognize(&image, true);
    let det = recognition.detections.len();
    let bd = recognition.detections.iter().filter(|d| d.is_bottom).count();
    let now = engine.started.elapsed().as_secs_f64();
    engine.tracker.observe(&recognition.detections, now);
    let snapshot = engine.tracker.snapshot();
    Some(encode(&engine.game, &snapshot, det, bd))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zhuweiyou_decktrace_NativeBridge_reset(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    let engine_mutex = unsafe { &*(handle as *const Mutex<Engine>) };
    if let Ok(mut engine) = engine_mutex.lock() {
        engine.tracker.reset();
        engine.started = Instant::now();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zhuweiyou_decktrace_NativeBridge_destroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    unsafe {
        drop(Box::from_raw(handle as *mut Mutex<Engine>));
    }
}

fn encode(game: &GameProfile, snapshot: &TrackerSnapshot, det: usize, bottom_det: usize) -> String {
    let remaining = game.remaining_card_ids(&snapshot.seen_ids);
    let bombs = bomb_ids(game, &remaining);
    let bottom: Vec<String> = snapshot.bottom_ids.iter().cloned().collect();
    format!(
        "armed:{};seen:{};remaining:{};det:{};bd:{};cards:{};bombs:{};bottom:{}",
        if snapshot.armed { 1 } else { 0 },
        snapshot.seen_count,
        snapshot.remaining,
        det,
        bottom_det,
        remaining.join(","),
        bombs.into_iter().collect::<Vec<_>>().join(","),
        bottom.join(","),
    )
}

fn bomb_ids(game: &GameProfile, remaining: &[String]) -> BTreeSet<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for id in remaining {
        *counts.entry(rank_of(id, game)).or_insert(0) += 1;
    }
    let mut result = BTreeSet::new();
    for (rank, count) in &counts {
        let total = game.rank_totals.get(rank).copied().unwrap_or(0) as usize;
        if total >= 4 && *count >= 4 {
            result.extend(
                remaining
                    .iter()
                    .filter(|id| rank_of(id, game) == *rank)
                    .cloned(),
            );
        }
    }
    if remaining.iter().any(|id| id == "BJ") && remaining.iter().any(|id| id == "SJ") {
        result.insert("BJ".to_string());
        result.insert("SJ".to_string());
    }
    result
}

fn rank_of(card_id: &str, game: &GameProfile) -> String {
    if game.labels.contains_key(card_id) {
        return card_id.to_string();
    }
    card_id
        .char_indices()
        .next_back()
        .map(|(idx, _)| card_id[..idx].to_string())
        .unwrap_or_else(|| card_id.to_string())
}
