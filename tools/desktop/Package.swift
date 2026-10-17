// swift-tools-version:5.9
// The macOS desktop helper taperec drives through `taperec-desktop <command>`.
import PackageDescription

let package = Package(
    name: "TaperecDesktop",
    platforms: [.macOS(.v13)],
    products: [.executable(name: "taperec-desktop", targets: ["taperec-desktop"])],
    targets: [
        .target(name: "DesktopKit"),
        .executableTarget(name: "taperec-desktop", dependencies: ["DesktopKit"]),
        .testTarget(name: "DesktopKitTests", dependencies: ["DesktopKit"], exclude: ["named-keys.txt"]),
    ]
)
