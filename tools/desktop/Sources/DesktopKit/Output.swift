// The tab-separated lines taperec parses; free of macOS so it tests anywhere.

/// A rectangle on screen in points, top-left origin.
public struct Frame: Equatable {
    public let x: Int, y: Int, width: Int, height: Int

    public init(x: Int, y: Int, width: Int, height: Int) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }
}

/// A labelled button as `buttons` reports it.
public struct Button: Equatable {
    public let app: String, frontmost: Bool, label: String, frame: Frame

    public init(app: String, frontmost: Bool, label: String, frame: Frame) {
        self.app = app
        self.frontmost = frontmost
        self.label = label
        self.frame = frame
    }
}

/// Keeps a field on one line and in one column.
func field(_ text: String) -> String {
    String(text.map { $0 == "\t" || $0.isNewline ? " " : $0 })
}

/// `app<TAB>frontmost<TAB>label<TAB>x<TAB>y<TAB>width<TAB>height` per button.
public func formatButtons(_ buttons: [Button]) -> String {
    buttons.map { b in
        [field(b.app), b.frontmost ? "true" : "false", field(b.label),
         String(b.frame.x), String(b.frame.y), String(b.frame.width), String(b.frame.height)]
            .joined(separator: "\t") + "\n"
    }.joined()
}

/// `name<TAB>x<TAB>y<TAB>width<TAB>height` per Dock item.
public func formatDock(_ items: [(name: String, frame: Frame)]) -> String {
    items.map { item in
        [field(item.name), String(item.frame.x), String(item.frame.y),
         String(item.frame.width), String(item.frame.height)].joined(separator: "\t") + "\n"
    }.joined()
}

/// `x<TAB>y<TAB>width<TAB>height` of a window, or nothing without one.
public func formatWindow(_ frame: Frame?) -> String {
    guard let frame else { return "" }
    return [frame.x, frame.y, frame.width, frame.height].map(String.init).joined(separator: "\t") + "\n"
}

/// One window title per line.
public func formatWindows(_ titles: [String]) -> String {
    titles.map { field($0) + "\n" }.joined()
}
