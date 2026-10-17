// What is on screen, and bringing apps forward, through the Accessibility API and NSWorkspace.
#if os(macOS)
import AppKit
import ApplicationServices

/// The longest one app may take to answer an accessibility query.
let appAnswerLimit: Float = 0.5

func copy(_ element: AXUIElement, _ attribute: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return nil }
    return value
}

func string(_ element: AXUIElement, _ attribute: String) -> String? {
    copy(element, attribute) as? String
}

func elements(_ element: AXUIElement, _ attribute: String) -> [AXUIElement] {
    (copy(element, attribute) as? [AXUIElement]) ?? []
}

func frame(_ element: AXUIElement) -> Frame? {
    guard let position = copy(element, kAXPositionAttribute), CFGetTypeID(position) == AXValueGetTypeID(),
          let size = copy(element, kAXSizeAttribute), CFGetTypeID(size) == AXValueGetTypeID()
    else { return nil }
    var point = CGPoint.zero
    var extent = CGSize.zero
    // Both values were checked to be AXValues above.
    guard AXValueGetValue(position as! AXValue, .cgPoint, &point),
          AXValueGetValue(size as! AXValue, .cgSize, &extent)
    else { return nil }
    return Frame(x: Int(point.x), y: Int(point.y), width: Int(extent.width), height: Int(extent.height))
}

/// An app's accessibility element, answering within [`appAnswerLimit`].
func element(of app: NSRunningApplication) -> AXUIElement {
    let element = AXUIElementCreateApplication(app.processIdentifier)
    AXUIElementSetMessagingTimeout(element, appAnswerLimit)
    return element
}

/// Titles of the windows of regular apps.
func windowTitles() -> [String] {
    NSWorkspace.shared.runningApplications
        .filter { $0.activationPolicy == .regular }
        .flatMap { app in elements(element(of: app), kAXWindowsAttribute).compactMap { string($0, kAXTitleAttribute) } }
        .filter { !$0.isEmpty }
}

/// Labelled buttons of every app's windows, including groups and sheets two levels down.
func onScreenButtons() -> [Button] {
    var found: [Button] = []
    for app in NSWorkspace.shared.runningApplications {
        let name = app.localizedName ?? ""
        for window in elements(element(of: app), kAXWindowsAttribute) {
            collectButtons(in: window, depth: 1, app: name, frontmost: app.isActive, into: &found)
        }
    }
    return found
}

func collectButtons(in element: AXUIElement, depth: Int, app: String, frontmost: Bool, into found: inout [Button]) {
    for child in elements(element, kAXChildrenAttribute) {
        let role = string(child, kAXRoleAttribute)
        if role == kAXButtonRole, let label = string(child, kAXTitleAttribute), !label.isEmpty,
           let frame = frame(child) {
            found.append(Button(app: app, frontmost: frontmost, label: label, frame: frame))
        } else if depth < 3, role == kAXGroupRole || role == kAXSheetRole {
            collectButtons(in: child, depth: depth + 1, app: app, frontmost: frontmost, into: &found)
        }
    }
}

/// The Dock's items by title and frame.
func dockItems() -> [(name: String, frame: Frame)] {
    guard let dock = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.dock").first
    else { return [] }
    let list = elements(element(of: dock), kAXChildrenAttribute).first { string($0, kAXRoleAttribute) == kAXListRole }
    guard let list else { return [] }
    return elements(list, kAXChildrenAttribute).compactMap { item in
        guard let name = string(item, kAXTitleAttribute), let frame = frame(item) else { return nil }
        return (name, frame)
    }
}

/// Where apps are installed, the folders taperec's action looks in too.
let appFolders = ["/Applications", "/System/Applications", "/System/Applications/Utilities"]

/// A running app called `name`, by its name or its bundle's file name.
func runningApp(named name: String) -> NSRunningApplication? {
    NSWorkspace.shared.runningApplications.first {
        $0.localizedName == name || $0.bundleURL?.lastPathComponent == "\(name).app"
    }
}

/// Brings `name` to the front, starting it if needed; true once it, or an app inside its bundle, is frontmost.
func focusApp(named name: String) -> Bool {
    guard let app = runningApp(named: name) else {
        let installed = appFolders.map { URL(fileURLWithPath: $0).appendingPathComponent("\(name).app") }
            .first { FileManager.default.fileExists(atPath: $0.path) }
        if let installed {
            NSWorkspace.shared.openApplication(at: installed, configuration: NSWorkspace.OpenConfiguration())
        }
        return false
    }
    app.activate(options: [.activateAllWindows])
    // A background tool may not activate other apps; the accessibility route can.
    AXUIElementSetAttributeValue(element(of: app), kAXFrontmostAttribute as CFString, kCFBooleanTrue)
    guard let front = NSWorkspace.shared.frontmostApplication else { return false }
    guard let frontPath = front.bundleURL?.standardizedFileURL.path,
          let wanted = app.bundleURL?.standardizedFileURL.path
    else { return front.processIdentifier == app.processIdentifier }
    return frontPath == wanted || frontPath.hasPrefix(wanted + "/")
}
#endif
