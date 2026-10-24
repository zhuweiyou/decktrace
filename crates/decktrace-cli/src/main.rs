use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use decktrace_app::{
    RunOptions, capture_one, run_once_live, run_once_on_image, run_tui, timestamp_slug,
};
use decktrace_capture::Platform;
use decktrace_game_api::GameProfile;
use decktrace_games::{get_game, list_games};
use decktrace_ui_tui::{TuiRenderer, render_text};

#[derive(Parser, Debug)]
#[command(name = "decktrace", about = "斗地主记牌器", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 列出已注册游戏
    Games,
    /// 列出窗口
    List(ListArgs),
    /// 截取一张窗口截图
    Shot(ShotArgs),
    /// 持续采集截图，不识别
    Watch(WatchArgs),
    /// 实时识别并显示记牌表
    Run(RunArgs),
}

#[derive(Args, Debug, Clone)]
struct WindowArgs {
    #[arg(long, default_value = "jj", help = "游戏 profile，默认 jj")]
    game: String,
    #[arg(long, help = "覆盖游戏窗口标题")]
    title: Option<String>,
    #[arg(long, help = "覆盖所属应用名")]
    owner: Option<String>,
    #[arg(long, help = "直接指定窗口 ID")]
    window_id: Option<u32>,
    #[arg(long, help = "包含离屏窗口")]
    include_offscreen: bool,
}

#[derive(Args, Debug)]
struct ListArgs {
    #[command(flatten)]
    window: WindowArgs,
    #[arg(long, help = "包含离屏窗口")]
    all: bool,
}

#[derive(Args, Debug)]
struct ShotArgs {
    #[command(flatten)]
    window: WindowArgs,
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct WatchArgs {
    #[command(flatten)]
    window: WindowArgs,
    #[arg(long, default_value_t = 0.5)]
    interval: f64,
    #[arg(long)]
    duration: Option<f64>,
    #[arg(long)]
    max_frames: Option<u64>,
    #[arg(long, default_value = "frames")]
    out_dir: PathBuf,
    #[arg(long)]
    save_all: bool,
}

#[derive(Args, Debug)]
struct RunArgs {
    #[command(flatten)]
    window: WindowArgs,
    #[arg(long, help = "只识别一帧")]
    once: bool,
    #[arg(long, help = "单帧模式直接读取图片")]
    image: Option<PathBuf>,
    #[arg(long)]
    duration: Option<f64>,
    #[arg(long)]
    debug: bool,
    #[arg(long, default_value_t = 2)]
    min_hits: usize,
    #[arg(long)]
    no_auto_reset: bool,
    #[arg(long, default_value = "frames")]
    out_dir: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Games => cmd_games(),
        Command::List(args) => cmd_list(args),
        Command::Shot(args) => cmd_shot(args),
        Command::Watch(args) => cmd_watch(args),
        Command::Run(args) => cmd_run(args),
    }
}

fn cmd_games() -> Result<()> {
    for game in list_games() {
        println!(
            "{:<8} {}  窗口={:?}  牌数={}  模板={}",
            game.id,
            game.name,
            game.window.title,
            game.total_cards(),
            game.templates_dir.display()
        );
    }
    Ok(())
}

fn cmd_list(args: ListArgs) -> Result<()> {
    let game = get_game(&args.window.game).map_err(anyhow::Error::msg)?;
    let platform = create_platform()?;
    let mut windows = platform
        .window_service()
        .list_windows(args.all || args.window.include_offscreen)?;
    windows.retain(|window| {
        window.layer == 0
            && window.width >= 80.0
            && window.height >= 80.0
            && (args.all || window.onscreen)
    });
    windows.sort_by(|a, b| (!a.onscreen, &a.app, &a.title).cmp(&(!b.onscreen, &b.app, &b.title)));
    if windows.is_empty() {
        println!("没有找到可见窗口。");
        return Ok(());
    }
    println!(
        "  {:>6}  {:<20} {:<32} {:>10}  {:>18}  {}",
        "ID", "应用", "标题", "尺寸", "位置", "在屏"
    );
    for window in &windows {
        let marker = if window.title.trim() == game.window.title {
            "→"
        } else {
            " "
        };
        println!(
            "{marker} {:>6}  {:<20} {:<32} {:>10}  {:>18}  {}",
            window.id,
            window.app,
            window.title,
            window.size(),
            window.position(),
            if window.onscreen { "是" } else { "否" }
        );
    }
    println!(
        "\n共 {} 个窗口；→ 表示标题匹配游戏 {:?}。",
        windows.len(),
        game.name
    );
    Ok(())
}

fn cmd_shot(args: ShotArgs) -> Result<()> {
    let (game, platform, window) = resolve_window(&args.window)?;
    let out = args.out.unwrap_or_else(|| {
        PathBuf::from("shots").join(format!("{}_{}.png", game.id, timestamp_slug()))
    });
    capture_one(platform.as_ref(), &window, &out)?;
    println!("窗口：{window}");
    println!("截图：{}", out.display());
    Ok(())
}

fn cmd_watch(args: WatchArgs) -> Result<()> {
    let (_game, platform, window) = resolve_window(&args.window)?;
    std::fs::create_dir_all(&args.out_dir)?;
    let fps = 1.0 / args.interval.max(0.02);
    let mut stream = platform.create_capture_stream(&window, fps as f32)?;
    stream.start()?;
    println!("窗口：{window}");
    println!(
        "采集后端：{}   输出：{}",
        stream.backend_name(),
        args.out_dir.display()
    );
    println!("按 Ctrl+C 停止。");
    let started = Instant::now();
    let mut frames = 0u64;
    while let Some(frame) = stream.read(Duration::from_secs(2))? {
        frames += 1;
        if args.save_all {
            let path = args
                .out_dir
                .join(format!("{}_{}.png", timestamp_slug(), frame.seq));
            frame.image.save(path)?;
        }
        frame.image.save(args.out_dir.join("latest.png"))?;
        if let Some(max) = args.max_frames {
            if frames >= max {
                break;
            }
        }
        if let Some(duration) = args.duration {
            if started.elapsed().as_secs_f64() >= duration {
                break;
            }
        }
    }
    stream.stop();
    let elapsed = started.elapsed().as_secs_f64().max(1e-6);
    println!("采集结束：{frames} 帧，{:.1} fps", frames as f64 / elapsed);
    Ok(())
}

fn cmd_run(args: RunArgs) -> Result<()> {
    let (game, platform, window) = resolve_window(&args.window)?;
    if args.once {
        let result = if let Some(path) = &args.image {
            run_once_on_image(&game, path, args.debug)?
        } else {
            run_once_live(platform.as_ref(), &game, &window, args.debug)?
        };
        let status = format!("单帧模式 · 识别 {:.0}ms", result.recognition_ms);
        let color = std::io::stdout().is_terminal();
        print!("{}", render_text(&result.snapshot, &game, &status, color));
        return Ok(());
    }

    let renderer = Box::new(TuiRenderer::new(std::io::stdout().is_terminal()))
        as Box<dyn decktrace_ui::Renderer>;
    println!("目标窗口：{window}");
    println!("游戏：{}   输出：{}", game.name, args.out_dir.display());
    println!("按 r 重置本局，q 或 Ctrl+C 退出。");
    run_tui(
        platform.as_ref(),
        game,
        &window,
        renderer,
        RunOptions {
            auto_reset: !args.no_auto_reset,
            min_hits: args.min_hits,
            debug: args.debug,
            duration: args.duration,
            out_dir: args.out_dir,
        },
    )
}

fn resolve_window(
    args: &WindowArgs,
) -> Result<(GameProfile, Box<dyn Platform>, decktrace_core::WindowInfo)> {
    let game = get_game(&args.game).map_err(anyhow::Error::msg)?.clone();
    let platform = create_platform()?;
    let mut selector = game.window.clone();
    if let Some(title) = &args.title {
        selector.title = title.clone();
    }
    if let Some(owner) = &args.owner {
        selector.app = Some(owner.clone());
    }
    if let Some(id) = args.window_id {
        selector.window_id = Some(id);
    }
    selector.include_off_screen = args.include_offscreen;
    let window = platform
        .window_service()
        .find_window(&selector)
        .context("查找目标窗口失败")?;
    Ok((game, platform, window))
}

#[cfg(target_os = "macos")]
fn create_platform() -> Result<Box<dyn Platform>> {
    Ok(Box::new(decktrace_capture_macos::MacPlatform::new()?))
}

#[cfg(not(target_os = "macos"))]
fn create_platform() -> Result<Box<dyn Platform>> {
    Err(anyhow!("当前只实现了 macOS 采集后端"))
}
