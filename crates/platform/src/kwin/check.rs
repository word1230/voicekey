//! 「规则装完了，KWin 真的照做了吗」——把两边对着比，给出逐条结论。
//!
//! 纯逻辑：读文件、问 KWin 都在调用方（`crates/platform/src/bin/vk-kwin.rs`），
//! 这里只判定，好在单测里拿真跑出来的数据喂它（见本文件末尾那两条实测基线）。
//!
//! 为什么值得单独跑一遍：KWin 对认不出的键名和不对的匹配是**静默**的——规则看着装上了，
//! 窗口该什么样还什么样。`keepabove`（不是 KWin 的键）与把 Force 当 Apply 都这么踩过。

use crate::kwin::{IniGroup, Rule, WindowState};

/// 一条验收结论
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// 检查项，打印出来就是一行
    pub item: String,
    pub outcome: Outcome,
    /// 不通过（或跳过）时说明为什么
    pub why: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    /// 没通过：规则不对，或 KWin 没照做
    Fail,
    /// 没东西可验：浮层此刻没映射。默认不因此报错，`--require-overlays` 则算失败
    Skip,
}

impl Verdict {
    fn pass(item: impl Into<String>) -> Self {
        Verdict { item: item.into(), outcome: Outcome::Pass, why: String::new() }
    }

    fn fail(item: impl Into<String>, why: impl Into<String>) -> Self {
        Verdict { item: item.into(), outcome: Outcome::Fail, why: why.into() }
    }

    fn skip(item: impl Into<String>, why: impl Into<String>) -> Self {
        Verdict { item: item.into(), outcome: Outcome::Skip, why: why.into() }
    }
}

/// 一项检查：过了就记一笔，没过就带上为什么
fn check(item: String, ok: bool, why: String) -> Verdict {
    if ok {
        Verdict::pass(item)
    } else {
        Verdict::fail(item, why)
    }
}

/// 一个浮层该被摆成什么样
pub struct Expected<'a> {
    /// 规则里 `wmclass` 要比的字符串
    pub app_id: &'a str,
    /// 窗口标题，也是输出里那一行的名字（`hud` / `pick`）
    pub title: &'a str,
    /// 期望的窗口左上角（逻辑像素）。`None` = 位置交给 KWin 默认放置，不验（pick 那条）
    pub pos: Option<(i64, i64)>,
    /// 期望的窗口尺寸：高度 `None` = 随内容变，只验宽度（pick）
    pub size: (i64, Option<i64>),
}

/// 静态层：`kwinrulesrc` 里那两组 vs 现在该有的两组。
///
/// 组不在、键少一个、键多一个、值不一样，都算不过。多出来的键尤其要看：KWin 6 会把老键名
/// （`noborder`）迁移成新键名**并回写文件**，这条就是这样抓到的。
pub fn rules_verdicts(installed: &[IniGroup], expected: &[Rule]) -> Vec<Verdict> {
    let mut out = Vec::new();
    // KWin 按 `[General] Order` 认规则清单：组名不在里面 = 规则写了也白写
    let order = installed.iter().find(|g| g.name == "General").and_then(|g| g.value("Order")).unwrap_or_default();
    let listed: Vec<&str> = order.split(',').map(str::trim).collect();
    for r in expected {
        out.push(check(
            format!("Order 里有 {}", r.group),
            listed.contains(&r.group),
            format!("`[General] Order={order}` 里没有它——KWin 只认 Order 列出的组"),
        ));
        let Some(g) = installed.iter().find(|g| g.name == r.group) else {
            out.push(Verdict::fail(format!("{}：组在文件里", r.group), "没装过，或者被删了：跑 vk-kwin install"));
            continue;
        };
        for (k, v) in r.entries() {
            let item = format!("{}：{k}", r.group);
            out.push(match g.value(k) {
                Some(got) if got == v => Verdict::pass(item),
                Some(got) => Verdict::fail(item, format!("文件里是 `{got}`，该是 `{v}`")),
                None => Verdict::fail(item, format!("文件里没这个键，该是 `{v}`")),
            });
        }
        let known: Vec<&str> = r.entries().iter().map(|(k, _)| *k).collect();
        for k in g.keys() {
            if !known.contains(&k) {
                out.push(Verdict::fail(
                    format!("{}：多了个 {k}", r.group),
                    format!("文件里有 `{k}={}`，我们没写过这个键——KWin 迁移过老键名，还是有人手改过？", g.value(k).unwrap_or("")),
                ));
            }
        }
    }
    out
}

/// 行为层：KWin 现在把这扇浮层摆成什么样 vs 规则要的。
///
/// `w` 为 `None` = 此刻没映射，记一条**跳过**而不是通过：没东西可看就别报绿。
pub fn overlay_verdicts(w: Option<&WindowState>, expect: &Expected) -> Vec<Verdict> {
    let name = expect.title;
    let Some(w) = w else {
        return vec![Verdict::skip(
            format!("{name} 浮层"),
            // 两者的出场条件不同：hud 按住热键就映射，pick 要等候选列表出来
            format!(
                "此刻没映射：{}再跑（或加 --require-overlays 把这条当失败）",
                if name == crate::kwin::PICK_TITLE { "等候选列表出来" } else { "按住热键让它出现" }
            ),
        )];
    };
    let mut out = vec![
        check(
            format!("{name}：app_id 是 {}", expect.app_id),
            w.class == expect.app_id,
            format!("KWin 报的是 `{}`——`wmclass` 比不中，整条规则都不生效", w.class),
        ),
        check(
            format!("{name}：不抢键盘焦点"),
            !w.wants_input && !w.active,
            format!("active={} wantsInput={}——任一为真，正在打的字就断了", w.active, w.wants_input),
        ),
        check(format!("{name}：无标题栏 / 边框 / 阴影"), w.no_border, "noBorder=false：合成器还在给它画装饰".into()),
        check(
            format!("{name}：始终置顶"),
            w.keep_above,
            format!("keepAbove=false（layer={}）——别的窗口能盖住它", w.layer),
        ),
        check(format!("{name}：不进任务栏"), w.skip_taskbar, "skipTaskbar=false".into()),
    ];
    let want_h = expect.size.1;
    let size_ok = w.w == expect.size.0 && want_h.is_none_or(|h| w.h == h);
    let want = match want_h {
        Some(h) => format!("{}x{h}", expect.size.0),
        None => format!("宽 {}", expect.size.0),
    };
    out.push(check(
        format!("{name}：窗口就是内容那 {want}"),
        size_ok,
        format!(
            "实际 {}x{}——高度不对多半是前端没在渲染（dev 构建加载 devUrl），见 docs/linux.md",
            w.w, w.h
        ),
    ));
    if let Some((x, y)) = expect.pos {
        out.push(check(
            format!("{name}：贴底居中（{x},{y}）"),
            (w.x, w.y) == (x, y),
            format!("实际 {},{}", w.x, w.y),
        ));
    }
    out
}

/// 该让 `check` 退非零的那些结论：失败总是算；跳过只在要求「必须验到」时才算
pub fn failures(vs: &[Verdict], require_overlays: bool) -> Vec<&Verdict> {
    vs.iter()
        .filter(|v| v.outcome == Outcome::Fail || (require_overlays && v.outcome == Outcome::Skip))
        .collect()
}

/// 打印用标签，以及「这条是 required 却跳过了」的标记。
///
/// `--require-overlays` 下被计入失败的 Skip 也显示成 FAIL：摘要会说「上面带 FAIL 的就是」，
/// 不能让人去找一行不存在的 FAIL。
pub fn label(v: &Verdict, require_overlays: bool) -> (&'static str, bool) {
    match v.outcome {
        Outcome::Pass => ("ok", false),
        Outcome::Fail => ("FAIL", false),
        Outcome::Skip if require_overlays => ("FAIL", true),
        Outcome::Skip => ("skip", false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kwin::{hud_rule, parse_dump, parse_ini, pick_rule};
    use crate::overlay::Area;

    /// KWin `KWin.PlacementArea`（逻辑像素）：本机 1920×1080，顶部 44px 面板
    fn work_area() -> Area {
        Area { x: 0.0, y: 44.0, w: 1920.0, h: 1036.0, scale: 1.0, physical: false }
    }

    /// 装好之后 `kwinrulesrc` 该有的样子：把我们生成的两组排成文件
    fn installed_file() -> String {
        let mut s = format!("[General]\nOrder=steam,{}\n\n", crate::kwin::MANAGED_GROUPS.join(","));
        for r in [hud_rule("voicekey", &work_area()), pick_rule("voicekey")] {
            s.push_str(&r.ini());
            s.push('\n');
        }
        s
    }

    fn verdicts_for(text: &str) -> Vec<Verdict> {
        let expected = vec![hud_rule("voicekey", &work_area()), pick_rule("voicekey")];
        rules_verdicts(&parse_ini(text), &expected)
    }

    /// 自己刚装完的文件：一条不过都没有（否则这个检查没法用）
    #[test]
    fn a_freshly_installed_file_passes_every_rule_check() {
        let vs = verdicts_for(&installed_file());
        let bad: Vec<&str> = vs.iter().filter(|v| v.outcome == Outcome::Fail).map(|v| v.item.as_str()).collect();
        assert!(bad.is_empty(), "不该有不过的：{bad:?}");
        // 两条规则各 1 条 Order + 各自 15 / 13 个键，都在
        assert_eq!(vs.iter().filter(|v| v.outcome == Outcome::Pass).count(), vs.len());
    }

    /// 本轮真踩过的那个值：`*rule=2`（Force）当成 Apply 写了——值合法、语义不同，只有比对才发现
    #[test]
    fn a_wrong_policy_value_is_reported_with_both_values() {
        let text = installed_file().replace("positionrule=3", "positionrule=2");
        let vs = verdicts_for(&text);
        let v = vs.iter().find(|v| v.item == "voicekey-hud：positionrule").expect("该报这一条");
        assert_eq!(v.outcome, Outcome::Fail);
        assert!(v.why.contains("`2`") && v.why.contains("`3`"), "why 该把两边都摆出来：{}", v.why);
    }

    /// KWin 迁移老键名会回写文件，于是同一条规则里多出几个我们没写过的键——必须报出来
    #[test]
    fn a_key_kwin_migrated_shows_up_as_extra() {
        let mut text = installed_file();
        text = text.replace("[voicekey-hud]\n", "[voicekey-hud]\nnoborder=true\nnoborderrule=2\n");
        let vs = verdicts_for(&text);
        let extra: Vec<&str> = vs.iter().filter(|v| v.outcome == Outcome::Fail).map(|v| v.item.as_str()).collect();
        assert!(extra.contains(&"voicekey-hud：多了个 noborder"), "{extra:?}");
        assert!(extra.contains(&"voicekey-hud：多了个 noborderrule"), "{extra:?}");
    }

    /// 没卸载干净 / 有人把组名从 Order 里删了：规则在文件里但 KWin 不认
    #[test]
    fn a_group_missing_from_order_is_reported() {
        let vs = verdicts_for(&installed_file().replace("Order=steam,voicekey-hud,voicekey-pick", "Order=steam"));
        let v = vs.iter().find(|v| v.item == "Order 里有 voicekey-hud").expect("该报这一条");
        assert_eq!(v.outcome, Outcome::Fail);
        assert!(v.why.contains("Order=steam"), "why 该把现状摆出来：{}", v.why);
    }

    /// 没装过：两组都不在，且不是崩掉，而是逐条说不通过
    #[test]
    fn an_empty_file_fails_with_actionable_items() {
        let vs = verdicts_for("[General]\nOrder=steam\n");
        let fails = failures(&vs, false);
        assert!(fails.iter().any(|v| v.why.contains("vk-kwin install")), "{:#?}", fails);
        assert!(fails.iter().all(|v| !v.why.is_empty()));
    }

    /// 实测基线（`docs/linux.md` 的验证记录）：装上规则后按住热键，KWin 读回来的 HUD
    const RULED_HUD: &str = "VKD|voicekey|620,936,680,120|false|false|true|true|true|3|hud";
    /// 同一扇窗口**未装规则**时的实测值：屏幕正中、抢焦点、layer=2
    const UNRULED_HUD: &str = "VKD|voicekey|620,502,680,120|true|true|false|false|false|2|hud";

    fn hud_expected() -> Expected<'static> {
        Expected { app_id: "voicekey", title: "hud", pos: Some((620, 936)), size: (680, Some(120)) }
    }

    #[test]
    fn the_recorded_ruled_hud_passes() {
        let d = parse_dump(RULED_HUD);
        let vs = overlay_verdicts(d.first(), &hud_expected());
        let bad: Vec<&str> = failures(&vs, true).iter().map(|v| v.item.as_str()).collect();
        assert!(bad.is_empty(), "实测的装上规则那条该全过：{bad:?}");
    }

    /// 反向基线：拿未装规则时的实测值喂进来，必须掉在焦点 / 装饰 / 置顶 / 位置上
    #[test]
    fn the_recorded_unruled_hud_fails_on_every_rule_item() {
        let d = parse_dump(UNRULED_HUD);
        let vs = overlay_verdicts(d.first(), &hud_expected());
        let bad: Vec<&str> = failures(&vs, true).iter().map(|v| v.item.as_str()).collect();
        assert_eq!(
            bad,
            [
                "hud：不抢键盘焦点",
                "hud：无标题栏 / 边框 / 阴影",
                "hud：始终置顶",
                "hud：不进任务栏",
                "hud：贴底居中（620,936）"
            ],
            "该只在这五条上掉：尺寸与 app_id 本来就对（未装规则时窗口自己也还是 680×120）"
        );
    }

    /// pick 不验位置（它的高度随候选行数变），但尺寸那条仍要看宽度
    #[test]
    fn pick_is_checked_for_attributes_but_not_for_position() {
        let expect = Expected { app_id: "voicekey", title: "pick", pos: None, size: (520, None) };
        let d = parse_dump("VKD|voicekey|700,362,520,400|false|false|true|true|true|3|pick");
        let items: Vec<String> = overlay_verdicts(d.first(), &expect).iter().map(|v| v.item.clone()).collect();
        assert!(!items.iter().any(|i| i.contains("贴底居中")), "{items:?}");
        assert!(items.iter().any(|i| i.contains("宽 520")), "{items:?}");
    }

    /// 浮层没映射时是「跳过」，不是通过；要求必须验到时才算失败
    #[test]
    fn an_unmapped_overlay_is_skipped_unless_overlays_are_required() {
        let vs = overlay_verdicts(None, &hud_expected());
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].outcome, Outcome::Skip);
        assert!(failures(&vs, false).is_empty(), "默认不该因此退非零");
        assert_eq!(failures(&vs, true).len(), 1, "--require-overlays 时要算失败");
        // 摘要说「上面带 FAIL 的就是」，所以 required 的 skip 必须真的显示成 FAIL
        assert_eq!(label(&vs[0], false), ("skip", false));
        assert_eq!(label(&vs[0], true), ("FAIL", true));
    }
}
