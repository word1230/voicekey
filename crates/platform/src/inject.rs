//! 上屏前提：注入设备此刻可不可写。
//!
//! Linux 上把字写进前台应用要经过 `/dev/uinput` 这把虚拟键盘；设备不可写时上屏**不报错**，
//! 只是什么都不发生——症状是"说了话，字没上屏"。所以启动时先探一次，把结论直接讲给用户。
//!
//! 探测是 IO（[`crate::inject_device`]，各平台一份），**判定与文案在这里是纯函数**，可单测。

/// 注入设备节点（Linux）。macOS / Windows 不走设备节点，那边恒为 [`Device::Writable`]。
pub const DEVICE_PATH: &str = "/dev/uinput";

/// 仓库自带的 udev 规则：文件在仓库里的位置，与它该装到哪儿。
///
/// 安装路径写在文案里也写在 `docs/linux.md` 的权限小节里——两边指的是同一个文件，
/// [`rule_paths_are_the_ones_the_docs_use`] 把它钉住。
pub const RULES_FILE: &str = "Scripts/udev/60-voicekey-uinput.rules";
pub const RULES_DEST: &str = "/etc/udev/rules.d/60-voicekey-uinput.rules";

/// 一次探测的结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    /// 存在且可写：能上屏
    Writable,
    /// 存在，但当前用户不可写：装上仓库自带的 udev 规则能修
    NotWritable,
    /// 设备节点不存在：uinput 没加载
    Missing,
}

/// 上屏不可用的原因（能上屏时是 `None`）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    NotWritable,
    Missing,
}

/// 判定：这一次探测下来能不能上屏，不能则是哪个原因
pub fn problem(d: Device) -> Option<Problem> {
    match d {
        Device::Writable => None,
        Device::NotWritable => Some(Problem::NotWritable),
        Device::Missing => Some(Problem::Missing),
    }
}

/// 给用户看的一句话：现在为什么上不了屏、怎么修。
///
/// 只说结论与修法，不说"现在无法上屏"这类判断——那句由界面（浮层 / 设置窗口 / CLI）
/// 顶在文案上面，同一个原因在三处不必写三遍。
pub fn advice(p: Problem) -> String {
    match p {
        Problem::NotWritable => format!(
            "{DEVICE_PATH} 不可写。装仓库自带的 udev 规则，然后让设备节点重建一次\
             （或重新登录）：sudo install -m 644 {RULES_FILE} {RULES_DEST} && \
             sudo udevadm control --reload-rules && sudo modprobe -r uinput && sudo modprobe uinput\
             （读按键不受影响）"
        ),
        Problem::Missing => format!(
            "设备节点 {DEVICE_PATH} 不存在，uinput 没加载。sudo modprobe uinput，\
             并让它开机加载：echo uinput | sudo tee /etc/modules-load.d/uinput.conf"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 谓词：只有「存在且可写」才算能上屏。另外两种都得报错——它们的症状一样
    /// （说了话字没上屏），混在一起报的话用户不知道去修哪一处。
    #[test]
    fn only_a_writable_device_is_ready() {
        assert_eq!(problem(Device::Writable), None);
        assert_eq!(problem(Device::NotWritable), Some(Problem::NotWritable));
        assert_eq!(problem(Device::Missing), Some(Problem::Missing));
    }

    /// 不可写时给的是「装仓库自带的那条 udev 规则」这条路：具体到文件与目标目录，
    /// 用户照着敲一条命令就行，不必去猜要写什么规则。（本机原先蹭的是 Steam 的规则，
    /// 卸掉 Steam 就没了——这条文案不指别的软件。）
    #[test]
    fn not_writable_advice_points_at_the_bundled_udev_rule() {
        let a = advice(Problem::NotWritable);
        assert!(a.contains(DEVICE_PATH), "{a}");
        assert!(a.contains(RULES_FILE), "{a}");
        assert!(a.contains(RULES_DEST), "{a}");
    }

    /// 规则文件的仓库位置与安装目标：文案、文档、文件本身都按这两个值对齐。
    /// 文档（`docs/linux.md` 的权限小节）与规则文件本身都提到这两个路径，改名字时一起改。
    #[test]
    fn rule_paths_are_the_ones_the_docs_use() {
        assert_eq!(RULES_FILE, "Scripts/udev/60-voicekey-uinput.rules");
        assert_eq!(RULES_DEST, "/etc/udev/rules.d/60-voicekey-uinput.rules");
    }

    /// 设备节点不存在是另一回事：uinput 没加载，装 udev 规则不解决它，修法是 modprobe
    #[test]
    fn missing_device_advice_points_at_loading_uinput() {
        let a = advice(Problem::Missing);
        assert!(a.contains(DEVICE_PATH), "{a}");
        assert!(a.contains("modprobe uinput"), "{a}");
        assert!(!a.contains(RULES_FILE), "设备不在时该修的是模块，不是权限：{a}");
    }
}
