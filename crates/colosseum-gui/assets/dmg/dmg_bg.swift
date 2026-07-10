// Render the Colosseum DMG background at 1x (640x400) and 2x (1280x800).
// Usage: swift dmg_bg.swift <inter-regular.otf> <outdir>
import Foundation
import CoreGraphics
import CoreText
import ImageIO
import UniformTypeIdentifiers

let interPath = CommandLine.arguments[1]
let outDir = CommandLine.arguments[2]

// Icon slots must match create-dmg: --icon-size 128, app at (160,200), drop link at (480,200).
// create-dmg coordinates are top-left origin, icon CENTERS at those points.
let W: CGFloat = 640, H: CGFloat = 400
let appX: CGFloat = 160, dropX: CGFloat = 480, slotY: CGFloat = 200  // top-left origin

func loadFont(_ path: String, size: CGFloat) -> CTFont {
    let url = URL(fileURLWithPath: path) as CFURL
    guard let descs = CTFontManagerCreateFontDescriptorsFromURL(url) as? [CTFontDescriptor],
          let d = descs.first else { fatalError("cannot load font \(path)") }
    return CTFontCreateWithFontDescriptor(d, size, nil)
}

func draw(scale: CGFloat, to path: String) {
    let w = Int(W * scale), h = Int(H * scale)
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8,
                        bytesPerRow: 0, space: cs,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.scaleBy(x: scale, y: scale)
    // Flip to top-left origin so layout matches create-dmg coordinates.
    ctx.translateBy(x: 0, y: H)
    ctx.scaleBy(x: 1, y: -1)

    // ── Background: subtle vertical gradient, warm paper (light — Finder always
    // draws icon labels dark over a background image, so the canvas must be light) ──
    let grad = CGGradient(colorsSpace: cs, colors: [
        CGColor(srgbRed: 0.962, green: 0.953, blue: 0.936, alpha: 1),  // #F5F3EF top
        CGColor(srgbRed: 0.918, green: 0.906, blue: 0.882, alpha: 1),  // #EAE7E1 bottom
    ] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(grad, start: CGPoint(x: 0, y: 0), end: CGPoint(x: 0, y: H), options: [])

    // ── Faint concentric-ring motif (echo of the logo), centered, behind everything ──
    let center = CGPoint(x: W / 2, y: slotY)
    for (r, a) in [(300.0, 0.060), (240.0, 0.080), (180.0, 0.100)] {
        ctx.setStrokeColor(CGColor(srgbRed: 0.62, green: 0.47, blue: 0.13, alpha: a))
        ctx.setLineWidth(1.5)
        ctx.strokeEllipse(in: CGRect(x: center.x - r, y: center.y - r, width: 2 * r, height: 2 * r))
    }

    // ── Soft circular "slot" hints under both icon positions ──
    for x in [appX, dropX] {
        ctx.setFillColor(CGColor(srgbRed: 0, green: 0, blue: 0, alpha: 0.030))
        let r: CGFloat = 78
        ctx.fillEllipse(in: CGRect(x: x - r, y: slotY - r, width: 2 * r, height: 2 * r))
        ctx.setStrokeColor(CGColor(srgbRed: 0, green: 0, blue: 0, alpha: 0.070))
        ctx.setLineWidth(1)
        ctx.strokeEllipse(in: CGRect(x: x - r, y: slotY - r, width: 2 * r, height: 2 * r))
    }

    // ── Gold arrow between the slots ──
    let gold = CGColor(srgbRed: 0.72, green: 0.55, blue: 0.14, alpha: 0.95)
    let y = slotY
    let x0: CGFloat = appX + 95, x1: CGFloat = dropX - 95   // clear of the 128px icons + slot rings
    let headL: CGFloat = 16, headW: CGFloat = 11
    ctx.setStrokeColor(gold)
    ctx.setLineWidth(3)
    ctx.setLineCap(.round)
    ctx.move(to: CGPoint(x: x0, y: y))
    ctx.addLine(to: CGPoint(x: x1 - headL + 4, y: y))
    ctx.strokePath()
    ctx.setFillColor(gold)
    ctx.move(to: CGPoint(x: x1, y: y))
    ctx.addLine(to: CGPoint(x: x1 - headL, y: y - headW))
    ctx.addLine(to: CGPoint(x: x1 - headL, y: y + headW))
    ctx.closePath()
    ctx.fillPath()

    // ── Hint text (Inter), centered near the bottom ──
    let font = loadFont(interPath, size: 13)
    let text = "Drag Colosseum into Applications to install"
    let attrs: [NSAttributedString.Key: Any] = [
        NSAttributedString.Key(kCTFontAttributeName as String): font,
        NSAttributedString.Key(kCTForegroundColorAttributeName as String):
            CGColor(srgbRed: 0.38, green: 0.36, blue: 0.32, alpha: 1),
    ]
    let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: attrs))
    let bounds = CTLineGetBoundsWithOptions(line, .useOpticalBounds)
    // CoreText draws in the default (bottom-left) coordinate system — un-flip locally.
    ctx.saveGState()
    ctx.translateBy(x: 0, y: H)
    ctx.scaleBy(x: 1, y: -1)
    ctx.textPosition = CGPoint(x: (W - bounds.width) / 2, y: H - 348)  // baseline at y=348 top-origin
    CTLineDraw(line, ctx)
    ctx.restoreGState()

    let img = ctx.makeImage()!
    let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL,
                                               UTType.png.identifier as CFString, 1, nil)!
    // Tag the DPI so Finder treats the 2x file as 144dpi (Retina).
    let props: [CFString: Any] = [kCGImagePropertyDPIWidth: 72 * scale, kCGImagePropertyDPIHeight: 72 * scale]
    CGImageDestinationAddImage(dest, img, props as CFDictionary)
    CGImageDestinationFinalize(dest)
    print("wrote \(path) (\(w)x\(h))")
}

draw(scale: 1, to: outDir + "/dmg-background.png")
draw(scale: 2, to: outDir + "/dmg-background@2x.png")
