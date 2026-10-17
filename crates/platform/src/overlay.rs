//! 浮层几何：给定屏幕工作区与浮层尺寸，算出浮层该出现在哪里。
//!
//! 应用（`ui.rs`）与 KWin 窗口规则生成共用这里的计算与尺寸常量，浮层位置因此只有一个真源：
//! Wayland 下客户端摆不了窗口，真实位置由合成器规则决定，所以两边必须算出同一个数。
//!
//! 坐标空间由 `Area::physical` 表明——macOS 是逻辑点（缩放不参与换算），
//! Windows / Linux 是物理像素（逻辑尺寸 × 缩放）。`Area` 的字段与 `bottom_center`
//! 的返回值都在这个空间里；窗口侧按空间选 `LogicalPosition` 还是 `PhysicalPosition`。

/// 胶囊浮层（hud）的逻辑尺寸
pub const HUD_W: f64 = 680.0;
pub const HUD_H: f64 = 120.0;
/// 候选面板（pick）的逻辑宽度
pub const PICK_W: f64 = 520.0;

/// 浮层底边离工作区底边的距离（逻辑尺寸）
pub const HUD_BOTTOM_MARGIN: f64 = 24.0;
/// 候选面板在插入点（caret）未知时同样落底部居中，但留得更开，免得压住输入行
pub const PICK_BOTTOM_MARGIN: f64 = 120.0;

/// 屏幕工作区（扣掉任务栏/面板后的可用区域）及其缩放因子
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
    /// 同 `Rect::physical`：true 是本空间用物理像素（Windows / Linux），false 是逻辑点（macOS）
    pub physical: bool,
}

impl Area {
    /// 逻辑尺寸 → 本工作区所在的坐标空间
    pub fn units(&self, size: f64) -> f64 {
        if self.physical { size * self.scale } else { size }
    }
}

/// 底部居中：水平居中于工作区，底边离工作区底边 `margin`。
///
/// `w` / `h` / `margin` 是逻辑尺寸，换算到工作区所在空间由函数自己完成。
/// 浮层比工作区大时不裁剪（居中优先，两侧对称溢出），需要贴边的调用者自己 clamp。
///
/// 工作区 1920×1080、缩放 1、浮层 680×120、边距 24 → `(620, 936)`。
pub fn bottom_center(area: &Area, w: f64, h: f64, margin: f64) -> (f64, f64) {
    (area.x + (area.w - area.units(w)) / 2.0, area.y + area.h - area.units(h) - area.units(margin))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 物理像素空间（Windows / Linux）的工作区
    fn physical(x: f64, y: f64, w: f64, h: f64, scale: f64) -> Area {
        Area { x, y, w, h, scale, physical: true }
    }

    /// 逻辑点空间（macOS）的工作区
    fn logical(x: f64, y: f64, w: f64, h: f64, scale: f64) -> Area {
        Area { x, y, w, h, scale, physical: false }
    }

    /// 本机 1920×1080、缩放 1：docs/linux.md 记的浮层落位，旧硬编码 (620,936) 的回归网
    #[test]
    fn hud_lands_bottom_center_on_1920x1080() {
        let a = physical(0.0, 0.0, 1920.0, 1080.0, 1.0);
        assert_eq!(bottom_center(&a, HUD_W, HUD_H, HUD_BOTTOM_MARGIN), (620.0, 936.0));
    }

    /// 面板在侧边：可用区域不从 (0,0) 开始，居中按工作区算而不是按屏幕算。
    /// 左侧面板 240 宽、底部面板 40 高 → 工作区 (240,0,1680,1040)
    #[test]
    fn centers_on_work_area_offset_by_panels() {
        let a = physical(240.0, 0.0, 1680.0, 1040.0, 1.0);
        assert_eq!(bottom_center(&a, 680.0, 120.0, 24.0), (740.0, 896.0));
    }

    /// 缩放 150% 的 Windows：浮层尺寸与边距都要乘缩放，工作区本身已是物理像素
    #[test]
    fn scales_overlay_size_into_physical_pixels() {
        let a = physical(0.0, 0.0, 2560.0, 1440.0, 1.5);
        assert_eq!(bottom_center(&a, 680.0, 120.0, 24.0), (770.0, 1224.0));
    }

    /// Retina macOS：工作区与浮层尺寸都是逻辑点，缩放 2.0 不参与换算
    #[test]
    fn logical_point_space_ignores_scale() {
        let a = logical(0.0, 0.0, 1440.0, 900.0, 2.0);
        assert_eq!(bottom_center(&a, 680.0, 120.0, 24.0), (380.0, 756.0));
    }

    /// 浮层比工作区宽：仍然居中（两侧对称溢出，x 落到工作区左边之外），不在这里裁剪——
    /// 需要贴边的是调用者（候选面板的 place_pick 随后自己 clamp）
    #[test]
    fn overlay_wider_than_work_area_still_centers() {
        let a = physical(0.0, 0.0, 600.0, 420.0, 1.0);
        assert_eq!(bottom_center(&a, 680.0, 120.0, 24.0), (-40.0, 276.0));
    }
}
