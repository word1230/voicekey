//! vk-kwin install|uninstall|probe —— KWin 浮层规则（贴底居中/置顶/不抢焦点）的幂等安装器
//!
//! 只面向 KWin Wayland：Wayland 下客户端摆不了窗口、去不掉装饰、定不了焦点，
//! 浮层的这三件事只能由合成器规则做（见 `docs/adr/0002`）。

#[cfg(target_os = "linux")]
fn main() {
    if let Err(e) = run() {
        eprintln!("vk-kwin: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("vk-kwin 只用于 Linux / KWin Wayland：别的平台浮层由应用自己摆");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), String> {
    use voicekey_platform::kwin;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str);
    let app_id_flag = args.iter().skip_while(|a| *a != "--app-id").nth(1).cloned();
    match cmd {
        Some("probe") => {
            let p = kwin::probe()?;
            match &p.area {
                Some(a) => println!("可用区域（KWin.PlacementArea，逻辑像素）：{},{} {}x{}", a.x, a.y, a.w, a.h),
                None => println!("可用区域：KWin 没报"),
            }
            println!("KWin 看到的窗口（app_id = resourceClass）：");
            for w in &p.windows {
                println!("  {:<32} pid={:<8} {}", w.class, w.pid, w.title);
            }
            match kwin::choose_app_id(&p.windows) {
                Some(id) => println!("据此会用的 app_id：{id}"),
                None => println!("据此选不出 app_id：浮层与设置窗口此刻都不在映射中"),
            }
            Ok(())
        }
        Some("install") => {
            let p = kwin::probe()?;
            let area = p.area.clone().ok_or("KWin 没报可用区域，算不出浮层该在哪")?;
            let (app_id, from) = resolve_app_id(app_id_flag, &p)?;
            let rules = kwin::install(&app_id, &area)?;
            println!("已装 KWin 规则（app_id={app_id}，{from}）：");
            if from == "上次装的" {
                println!("  注意：此刻 KWin 里没有 VoiceKey 窗口，沿用了上次装的那个 app_id；");
                println!("  若应用改过名/换过打包方式，先打开一次设置窗口（或按住热键让 HUD 出现）再重跑。");
            }
            for r in &rules {
                print!("{}", r.ini());
            }
            // 实测：重载只让「不抢焦点」（KWin 的 forceRule）对已映射的窗口当场生效，
            // 置顶 / 装饰 / 位置要等窗口自己重新映射——`vk-kwin check` 会把这种半吊子状态报出来
            println!("KWin 已重载：新开的浮层即刻生效。已经开着的浮层只有「不抢焦点」当场生效，");
            println!("其余几项要等它下次映射——重启一下应用，或者按住热键重开一次。");
            Ok(())
        }
        Some("dump") => {
            for s in kwin::dump()? {
                println!(
                    "{:<28} {:<10} {:>5},{:<5} {:>4}x{:<4} active={} wantsInput={} keepAbove={} noBorder={} skipTaskbar={} layer={}",
                    s.class, s.title, s.x, s.y, s.w, s.h, s.active, s.wants_input, s.keep_above, s.no_border, s.skip_taskbar, s.layer
                );
            }
            Ok(())
        }
        Some("check") => check(app_id_flag, args.iter().any(|a| a == "--require-overlays")),
        Some("uninstall") => {
            kwin::uninstall()?;
            println!("已卸载 KWin 规则（只删自己装的组，别人的规则没动），KWin 已重载");
            Ok(())
        }
        _ => {
            eprintln!(
                "用法：\n  \
                 vk-kwin install [--app-id <id>]  装规则（幂等；替换掉旧的手写规则）\n  \
                 vk-kwin uninstall                卸规则\n  \
                 vk-kwin check [--app-id <id>] [--require-overlays]\n                                   \
                 验收：文件里的规则 vs 该有的规则，以及 KWin 现在的表现\n  \
                 vk-kwin dump                     看 KWin 把每扇窗口摆成了什么样\n  \
                 vk-kwin probe                    看 KWin 现在有哪些窗口、app_id 会取哪个\n\n\
                 提示：install 需要浮层或设置窗口此刻在 KWin 里出现过一次，否则取不到 app_id；\n     \
                 check 的行为那半截要浮层在映射中（按住热键），没有就记一条「跳过」。"
            );
            std::process::exit(2);
        }
    }
}

/// 该用哪个 app_id：`--app-id` > 探针从 KWin 查得 > 上次装的那个（退路见 `installed_app_id`）
#[cfg(target_os = "linux")]
fn resolve_app_id(flag: Option<String>, p: &voicekey_platform::kwin::Probe) -> Result<(String, &'static str), String> {
    use voicekey_platform::kwin;
    if let Some(id) = flag {
        return Ok((id, "--app-id 指定"));
    }
    match kwin::choose_app_id(&p.windows) {
        Some(id) => Ok((id.to_string(), "KWin 查得")),
        // 应用没在跑（或浮层与设置窗口都没映射过）时，退回上次装的 app_id：
        // 重装/复验应当照旧，而不是因为一时看不到窗口就报错
        None => Ok((
            kwin::installed_app_id().ok_or(
                "没发现 VoiceKey 窗口，也没装过规则：先按住热键让 HUD 出现一次（或打开设置窗口），再重跑",
            )?,
            "上次装的",
        )),
    }
}

/// 验收：两个层次各给一条条结论。静态层看文件，行为层看 KWin 现在的表现。
///
/// 行为层要浮层在映射中（按住热键才会映射）：没有就记「跳过」而不是通过——没东西可看就别报绿。
/// `--require-overlays` 把跳过也算失败，给那种"必须验到"的场合。
#[cfg(target_os = "linux")]
fn check(app_id_flag: Option<String>, require_overlays: bool) -> Result<(), String> {
    use voicekey_platform::kwin;
    use voicekey_platform::kwin::check::{failures, overlay_verdicts, rules_verdicts, Expected, Outcome};
    use voicekey_platform::overlay::{bottom_center, HUD_BOTTOM_MARGIN, HUD_H, HUD_W, PICK_W};

    let p = kwin::probe()?;
    let area = p.area.clone().ok_or("KWin 没报可用区域：算不出浮层该在哪")?;
    let (app_id, _) = resolve_app_id(app_id_flag, &p)?;
    let expected = [kwin::hud_rule(&app_id, &area), kwin::pick_rule(&app_id)];

    let mut vs = rules_verdicts(&kwin::installed_groups()?, &expected);

    let states = kwin::dump()?;
    let find = |title: &str| states.iter().find(|w| w.class == app_id && w.title == title);
    let (x, y) = bottom_center(&area, HUD_W, HUD_H, HUD_BOTTOM_MARGIN);
    let hud = Expected {
        app_id: &app_id,
        title: kwin::HUD_TITLE,
        pos: Some((x.round() as i64, y.round() as i64)),
        size: (HUD_W as i64, Some(HUD_H as i64)),
    };
    // pick 的高度随候选行数变：不验位置、只验宽度
    let pick = Expected { app_id: &app_id, title: kwin::PICK_TITLE, pos: None, size: (PICK_W as i64, None) };
    vs.extend(overlay_verdicts(find(kwin::HUD_TITLE), &hud));
    vs.extend(overlay_verdicts(find(kwin::PICK_TITLE), &pick));

    for v in &vs {
        match v.outcome {
            Outcome::Pass => println!("  ok    {}", v.item),
            Outcome::Fail => println!("  FAIL  {} —— {}", v.item, v.why),
            Outcome::Skip => println!("  skip  {} —— {}", v.item, v.why),
        }
    }
    let count = |o: Outcome| vs.iter().filter(|v| v.outcome == o).count();
    let (passed, skipped) = (count(Outcome::Pass), count(Outcome::Skip));
    let tail = if skipped > 0 { format!("（{skipped} 项跳过未验）") } else { String::new() };
    match failures(&vs, require_overlays).len() {
        0 => {
            println!("{passed} 项全过{tail}");
            Ok(())
        }
        n => Err(format!("{n} 项没过（{passed} 项过{tail}）；上面带 FAIL 的就是")),
    }
}
