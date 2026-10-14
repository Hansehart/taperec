// Renders a warp caption to a PNG: taperec-caption <text> <out.png>
import AppKit

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("taperec-caption: \(message)\n".utf8))
    exit(1)
}

let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    fail("usage: taperec-caption <text> <out.png>")
}

let text = NSAttributedString(
    string: arguments[1],
    attributes: [.font: NSFont.boldSystemFont(ofSize: 44), .foregroundColor: NSColor.white]
)
let padding: CGFloat = 24
let size = text.size()
let width = Int((size.width + 2 * padding).rounded(.up))
let height = Int((size.height + 2 * padding).rounded(.up))

// Sized in pixels, so captions match any backing scale.
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
NSBezierPath(
    roundedRect: NSRect(x: 0, y: 0, width: width, height: height), xRadius: 16, yRadius: 16
).fill()
text.draw(at: NSPoint(x: padding, y: padding))
NSGraphicsContext.restoreGraphicsState()

guard let png = bitmap.representation(using: .png, properties: [:]) else {
    fail("cannot encode the caption as PNG")
}
do {
    try png.write(to: URL(fileURLWithPath: arguments[2]))
} catch {
    fail("cannot write \(arguments[2]): \(error)")
}
