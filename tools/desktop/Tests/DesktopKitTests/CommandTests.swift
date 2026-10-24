import XCTest
@testable import DesktopKit

final class CommandTests: XCTestCase {
    func testChordsParseLikeTapes() {
        XCTAssertEqual(Chord("return"), Chord("return"))
        XCTAssertEqual(Chord("return")?.key, .named("return"))
        XCTAssertEqual(Chord("primary+shift+p")?.modifiers, [.primary, .shift])
        XCTAssertEqual(Chord("primary+shift+p")?.key, .character("p"))
        XCTAssertEqual(Chord("ctrl+`")?.key, .character("`"))
        for bad in ["", "cmd+", "+p", "cmd++p", "Return", "ctrl+shift", "cmd+cmd+a", "hyper+a", "ab", "cmd+é"] {
            XCTAssertNil(Chord(bad), bad)
        }
    }

    func testCommandsParseTheirArguments() {
        XCTAssertEqual(Command(["move", "10", "20"]), .move(x: 10, y: 20))
        XCTAssertEqual(Command(["type", "x"]), .type("x"))
        XCTAssertEqual(Command(["paste", "git clone x"]), .paste("git clone x"))
        XCTAssertEqual(Command(["focus", "Visual Studio Code"]), .focus("Visual Studio Code"))
        XCTAssertEqual(Command(["record", "/out/demo.raw.mov"]), .record("/out/demo.raw.mov"))
        XCTAssertEqual(Command(["encoder"]), .encoder)
        XCTAssertEqual(Command(["window-at", "960", "540"]), .windowAt(x: 960, y: 540))
        for bad in [[], ["move", "1"], ["move", "-1", "2"], ["type", "ab"], ["focus", ""], ["jump"], ["click", "x"],
                    ["record"], ["record", ""], ["encoder", "x"], ["window-at", "1"], ["window-at", "1", "-2"]] {
            XCTAssertNil(Command(bad), "\(bad)")
        }
    }

    func testOutputsStayOneRecordPerLine() {
        let button = Button(app: "Core\tServices", frontmost: false, label: "Open\nnow",
                            frame: Frame(x: 963, y: 432, width: 112, height: 30))
        XCTAssertEqual(formatButtons([button]), "Core Services\tfalse\tOpen now\t963\t432\t112\t30\n")
        XCTAssertEqual(formatDock([(name: "Terminal", frame: Frame(x: 1, y: 2, width: 3, height: 4))]),
                       "Terminal\t1\t2\t3\t4\n")
        XCTAssertEqual(formatWindows(["a", "b"]), "a\nb\n")
        XCTAssertEqual(formatWindow(Frame(x: 10, y: 20, width: 300, height: 200)), "10\t20\t300\t200\n")
        XCTAssertEqual(formatWindow(nil), "")
    }

    func testNamedKeysMatchTaperec() throws {
        // taperec's Rust test keeps this copy of its NAMED_KEYS current.
        let url = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().appendingPathComponent("named-keys.txt")
        let rust = try String(contentsOf: url, encoding: .utf8).split(separator: "\n").map(String.init)
        XCTAssertEqual(namedKeys, rust)
    }
}
