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
            let (app_id, from) = match app_id_flag {
                Some(id) => (id, "--app-id 指定"),
                None => match kwin::choose_app_id(&p.windows) {
                    Some(id) => (id.to_string(), "KWin 查得"),
                    // 应用没在跑（或浮层与设置窗口都没映射过）时，退回上次装的 app_id：
                    // 重装应当照旧，而不是因为一时看不到窗口就报错
                    None => (
                        kwin::installed_app_id().ok_or(
                            "没发现 VoiceKey 窗口，也没装过规则：先按住热键让 HUD 出现一次（或打开设置窗口），再重跑",
                        )?,
                        "上次装的",
                    ),
                },
            };
            let rules = kwin::install(&app_id, &area)?;
            println!("已装 KWin 规则（app_id={app_id}，{from}）：");
            if from == "上次装的" {
                println!("  注意：此刻 KWin 里没有 VoiceKey 窗口，沿用了上次装的那个 app_id；");
                println!("  若应用改过名/换过打包方式，先打开一次设置窗口（或按住热键让 HUD 出现）再重跑。");
            }
            for r in &rules {
                print!("{}", r.ini());
            }
            println!("KWin 已重载，新开的浮层即刻生效（已经开着的窗口要等下次映射）");
            Ok(())
        }
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
                 vk-kwin probe                    看 KWin 现在有哪些窗口、app_id 会取哪个\n\n\
                 提示：install 需要浮层或设置窗口此刻在 KWin 里出现过一次，否则取不到 app_id。"
            );
            std::process::exit(2);
        }
    }
}
