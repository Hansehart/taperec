// Renders a PNG overlay: taperec-caption <text> <out.png> for a warp caption,
// taperec-caption --bar <text> <out.png> for the bar along the top of the video.
import AppKit

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("taperec-caption: \(message)\n".utf8))
    exit(1)
}

/// The bar's size in pixels: the video's full width.
let barWidth = 1920, barHeight = 64

var arguments = Array(CommandLine.arguments.dropFirst())
let bar = arguments.first == "--bar"
if bar { arguments.removeFirst() }
guard arguments.count == 2 else {
    fail("usage: taperec-caption [--bar] <text> <out.png>")
}

let text = NSAttributedString(
    string: arguments[0],
    attributes: [.font: NSFont.boldSystemFont(ofSize: bar ? 28 : 44), .foregroundColor: NSColor.white]
)
let padding: CGFloat = 24
let size = text.size()
let width = bar ? barWidth : Int((size.width + 2 * padding).rounded(.up))
let height = bar ? barHeight : Int((size.height + 2 * padding).rounded(.up))

// Sized in pixels, so overlays match any backing scale.
guard
    let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
    let context = NSGraphicsContext(bitmapImageRep: bitmap)
else {
    fail("cannot create a \(width)x\(height) bitmap")
}

NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = context
NSColor(white: 0, alpha: 0.6).setFill()
let area = NSRect(x: 0, y: 0, width: width, height: height)
if bar {
    area.fill()
    // Left-aligned and centred vertically.
    text.draw(at: NSPoint(x: padding, y: (CGFloat(height) - size.height) / 2))
} else {
    NSBezierPath(roundedRect: area, xRadius: 16, yRadius: 16).fill()
    text.draw(at: NSPoint(x: padding, y: padding))
}
NSGraphicsContext.restoreGraphicsState()

guard let png = bitmap.representation(using: .png, properties: [:]) else {
    fail("cannot encode the overlay as PNG")
}
do {
    try png.write(to: URL(fileURLWithPath: arguments[1]))
} catch {
    fail("cannot write \(arguments[1]): \(error)")
}
