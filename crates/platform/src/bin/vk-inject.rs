//! vk-inject —— 上屏这一段的两个自检：注入设备可不可写、以及真去上屏一次。
//!
//! 上屏失败是**静默**的（说了话，字没上屏，不报错），所以留一个能当场问一句、当场试一次的
//! 入口：查权限、装完 udev 规则后复验、不真说话就验一遍上屏链路、写进文档的验收清单，都跑它。
//! 应用启动时自己也会查（`perms.inject` → 设置窗口顶部那条，见 `docs/linux.md` 的权限小节）。
//!
//! 退出码：0 = 能上屏 / 试的那次发出去了；1 = 不能；2 = 用法错或不在 Linux 上。

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("vk-inject 只用于 Linux：macOS / Windows 的上屏不走设备节点，没有这一关");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() {
    use voicekey_platform::inject::{problem, DEVICE_PATH};
    use voicekey_platform::{inject_device, paste};

    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(a) = args.first() {
        if a == "-h" || a == "--help" {
            usage(0);
        }
        if a != "paste" {
            usage(2);
        }
        let Some(text) = args.get(1).filter(|t| !t.is_empty()) else {
            eprintln!("vk-inject paste 要一段文字：vk-inject paste 你好");
            std::process::exit(2);
        };
        // 先查前提：设备不可写时 paste 什么都不做，正是本项目要消灭的那种静默失败
        if let Some(p) = problem(inject_device()) {
            report(p);
        }
        // 借剪贴板 + uinput 的 Ctrl+V，与按住说话键说完话后的那一下是同一条路（`pf::paste`）。
        // `paste` 是 fire-and-forget（内部起线程），这里只等它把键发出去；**没发生成没成功它
        // 自己也不知道**——所以话里不说"上屏成功"，成功与否看焦点窗口。
        paste(text);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        println!("已把「{text}」交给剪贴板 + Ctrl+V（与应用上屏同一条路）；焦点窗口里没出现的话，看上面那两行");
        return;
    }

    match problem(inject_device()) {
        None => println!("{DEVICE_PATH}：可写\n能上屏"),
        Some(p) => report(p),
    }
}

/// 不能上屏时的结论：原因 + 怎么修。两处调用点共用。
#[cfg(target_os = "linux")]
fn report(p: voicekey_platform::inject::Problem) -> ! {
    use voicekey_platform::inject::advice;
    println!("现在无法上屏 —— {}", advice(p));
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn usage(code: i32) -> ! {
    println!(
        "用法：\n  \
         vk-inject                看注入设备（/dev/uinput）可不可写，也就是现在能不能上屏\n  \
         vk-inject paste <文字>    把文字上屏到当前焦点窗口 —— 真走一次上屏链路\n\
         \n\
         退出码：0 = 能上屏（试的那次也发出去了），1 = 不能。"
    );
    std::process::exit(code);
}
