// Runs one command and reports through stdout, stderr and the exit code.
import Foundation
#if os(macOS)
import ApplicationServices
import AppKit
#endif

func write(_ text: String, to handle: FileHandle) {
    handle.write(Data(text.utf8))
}

/// Runs the command in `arguments` (after the program name), returning the exit code.
public func run(_ arguments: [String]) -> Int32 {
    guard let command = Command(arguments) else {
        write(usage + "\n", to: .standardError)
        return Exit.failed.rawValue
    }
    #if os(macOS)
    return perform(command).rawValue
    #else
    write("taperec-desktop drives macOS only\n", to: .standardError)
    return Exit.failed.rawValue
    #endif
}

#if os(macOS)
/// Settling time after input, so events land before the process exits.
let settle: useconds_t = 20_000

func perform(_ command: Command) -> Exit {
    if case let .record(path) = command {
        record(to: path)
    }
    // Neither the encoder nor screen capture needs Accessibility.
    if command == .encoder {
        write(hardwareEncoder() ? "hardware\n" : "software\n", to: .standardOutput)
        return .ok
    }
    if command != .pointer, !AXIsProcessTrusted() {
        write("taperec-desktop needs Accessibility access: allow the program that runs it in "
              + "System Settings > Privacy & Security > Accessibility\n", to: .standardError)
        return .notTrusted
    }
    switch command {
    case .pointer:
        let at = pointerLocation()
        write("\(Int(at.x))\t\(Int(at.y))\n", to: .standardOutput)
    case let .move(x, y):
        moveMouse(to: CGPoint(x: x, y: y))
    case .click:
        clickMouse()
    case let .key(chord):
        pressChord(chord)
    case let .type(c):
        guard typeCharacter(c) else {
            write("cannot type \(c.debugDescription)\n", to: .standardError)
            return .untypable
        }
    case let .paste(text):
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        if let chord = Chord("primary+v") { pressChord(chord) }
    case .windows:
        write(formatWindows(windowTitles()), to: .standardOutput)
    case let .windowAt(x, y):
        write(formatWindow(windowFrame(atX: x, y: y)), to: .standardOutput)
    case .buttons:
        write(formatButtons(onScreenButtons()), to: .standardOutput)
    case .dock:
        let started = Date()
        do {
            write(formatDock(try dockItems()), to: .standardOutput)
        } catch {
            write("\(error) after \(Int(Date().timeIntervalSince(started) * 1000)) ms\n", to: .standardError)
            return .failed
        }
    case let .focus(name):
        write(focusApp(named: name) ? "true\n" : "false\n", to: .standardOutput)
    case .record, .encoder:
        break
    }
    usleep(settle)
    return .ok
}
#endif
