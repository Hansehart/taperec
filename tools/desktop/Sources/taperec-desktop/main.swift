// The macOS desktop helper: taperec-desktop <command> [arguments].
import DesktopKit
import Foundation

exit(run(Array(CommandLine.arguments.dropFirst())))
