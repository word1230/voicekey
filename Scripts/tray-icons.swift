// 生成 Tauri 菜单栏模板图标（macOS）。
// 用法：
//   swift Scripts/tray-icons.swift write [资源目录]   # 默认 app/src-tauri/icons
//
// 模板图标由 macOS 按 alpha 着色，因此素材用黑色 + 透明度绘制。
// tray-icon 会把图标缩放到 18pt 高，资产按 44px/18pt 输出。
// 只输出单张 tray.png：空闲与录音中共用「录音中」形态（App 原形单线波形）。

import AppKit

/// 与现有 44×44 资产保持一致的像素密度（44 / 18 ≈ 2.44x）。
let assetScale: CGFloat = 44.0 / 18.0

/// App 原形单线波形：包络与相位取自 App 图标的波形原形。
func appWavePath(
    from x0: CGFloat, to x1: CGFloat, centerY: CGFloat, amplitude: CGFloat,
    cycles: CGFloat, phase: CGFloat, envelopePower: CGFloat
) -> NSBezierPath {
    let path = NSBezierPath()
    let steps = 240
    for index in 0...steps {
        let t = CGFloat(index) / CGFloat(steps)
        let x = x0 + (x1 - x0) * t
        let envelope = pow(sin(.pi * t), envelopePower)
        let y = centerY + sin(2 * .pi * cycles * t + phase) * amplitude * envelope
        if index == 0 {
            path.move(to: NSPoint(x: x, y: y))
        } else {
            path.line(to: NSPoint(x: x, y: y))
        }
    }
    path.lineCapStyle = .round
    path.lineJoinStyle = .round
    return path
}

func render(tint: NSColor, scale: CGFloat) -> NSImage {
    let canvas = NSSize(width: 18, height: 18)
    let pixelWidth = Int((canvas.width * scale).rounded())
    let pixelHeight = Int((canvas.height * scale).rounded())
    guard let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: pixelWidth, pixelsHigh: pixelHeight,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    ) else {
        fatalError("无法创建位图")
    }

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let context = NSGraphicsContext.current!.cgContext
    context.scaleBy(x: scale, y: scale)
    context.setShouldAntialias(true)

    // 空闲与录音中共用「录音中」形态（用户确认更接近完整 App 图标）。
    let line = appWavePath(
        from: 1.7, to: 16.3, centerY: 10.14, amplitude: 5.2,
        cycles: 1.5, phase: 0.6, envelopePower: 2.0
    )
    line.lineWidth = 2.75
    tint.withAlphaComponent(1.0).setStroke()
    line.stroke()

    NSGraphicsContext.restoreGraphicsState()

    let image = NSImage(size: canvas)
    image.addRepresentation(rep)
    return image
}

func pngData(_ image: NSImage, pixelWidth: Int, pixelHeight: Int) -> Data {
    guard let tiff = image.tiffRepresentation,
          let rep = NSBitmapImageRep(data: tiff) else {
        fatalError("无法编码 PNG")
    }
    rep.size = NSSize(width: pixelWidth, height: pixelHeight)
    return rep.representation(using: .png, properties: [:])!
}

func write(_ image: NSImage, pixelWidth: Int, pixelHeight: Int, to url: URL) {
    try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    do {
        try pngData(image, pixelWidth: pixelWidth, pixelHeight: pixelHeight).write(to: url)
    } catch {
        fatalError("写入失败 \(url.path)：\(error)")
    }
}

let args = Array(CommandLine.arguments.dropFirst())
guard args.count >= 1, args[0] == "write" else {
    print("用法：swift Scripts/tray-icons.swift write [资源目录]（默认 app/src-tauri/icons）")
    exit(1)
}

let dir = URL(fileURLWithPath: args.count > 1 ? args[1] : "app/src-tauri/icons")
let image = render(tint: .black, scale: assetScale)
let url = dir.appendingPathComponent("tray.png")
write(image, pixelWidth: 44, pixelHeight: 44, to: url)
print("wrote \(url.path) (44x44)")
