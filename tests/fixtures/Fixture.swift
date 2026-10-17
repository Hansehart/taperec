// A window with known controls for the desktop contract test; it logs what reaches it.
import AppKit

let log = URL(fileURLWithPath: "/tmp/taperec-fixture.log")

func record(_ line: String) {
    let data = Data((line + "\n").utf8)
    if let handle = try? FileHandle(forWritingTo: log) {
        handle.seekToEndOfFile()
        handle.write(data)
        handle.closeFile()
    } else {
        try? data.write(to: log)
    }
}

final class Fixture: NSObject, NSApplicationDelegate {
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 480, height: 160),
                          styleMask: [.titled], backing: .buffered, defer: false)
    let field = NSTextField(frame: NSRect(x: 20, y: 100, width: 440, height: 24))

    func applicationDidFinishLaunching(_ notification: Notification) {
        window.title = "taperec fixture"
        for (index, label) in ["Cancel", "Open"].enumerated() {
            let button = NSButton(title: label, target: self, action: #selector(pressed(_:)))
            button.frame = NSRect(x: 260 + index * 100, y: 30, width: 90, height: 32)
            window.contentView?.addSubview(button)
        }
        field.target = self
        field.action = #selector(entered(_:))
        window.contentView?.addSubview(field)
        window.initialFirstResponder = field
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        record("ready")
    }

    @objc func pressed(_ sender: NSButton) {
        record("pressed \(sender.title)")
        window.makeFirstResponder(field)
    }

    /// Return in the field logs its text and clears it for the next line.
    @objc func entered(_ sender: NSTextField) {
        record("entered \(sender.stringValue)")
        sender.stringValue = ""
    }
}

let app = NSApplication.shared
let fixture = Fixture()
app.delegate = fixture
app.setActivationPolicy(.regular)
app.run()
