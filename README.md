# Packager

Packager turns a source file or a project folder into a macOS application or a command-line tool. The window is a SwiftUI app in `Packager.xcodeproj`. A Rust engine, bundled inside the app, detects the language and runs the toolchain.

## Open in Xcode

```bash
open Packager.xcodeproj
```

The Packager scheme builds the SwiftUI app. A build phase compiles the Rust engine and copies `packager-engine` into the app. Debug builds use `cargo build`. Release builds use `cargo build --release`.

## Build the app

You need Xcode and Rust.

```bash
./scripts/build-app.sh
open dist/Packager.app
```

The script builds the Packager scheme in Release and signs `dist/Packager.app`.

Redraw the app icon with `python3 scripts/make-icon.py`. That writes the image set in `Packager/Assets.xcassets`.

## Use it

1. Choose a source file or a project folder. You can drop it on the window.
2. Packager names the product and shows the language it found.
3. Choose **App** for a double-clickable `.app`, or **Tool** for a terminal program.
4. Click **Build**. The log streams while the toolchain runs.

The build uses the compilers installed on this Mac. A Finder-launched app still sees the tools from your login shell, including Homebrew and `~/.cargo/bin`.

Packaging writes its scratch files in a temporary directory. It does not add a `setup.py` to the project you selected.

## Languages

| Language | How a project is recognized | What you get |
| --- | --- | --- |
| Python | `.py`, `pyproject.toml`, `requirements.txt` | macOS app with py2app, or a PyInstaller tool |
| Rust | `Cargo.toml`, `.rs` | `cargo build --release`, or `rustc` for one file |
| Go | `go.mod`, `.go` | `go build` |
| JavaScript | `package.json`, `.js` | `bun --compile`, `pkg`, or a Node launcher |
| TypeScript | `tsconfig.json`, `.ts` | `bun build --compile` |
| Swift | `Package.swift`, `.swift` | `swift build -c release`, or `swiftc` |
| C | `.c`, CMake, Make | `clang`, CMake, or Make |
| C++ | `.cpp`, CMake, Make | `clang++`, CMake, or Make |
| Java | `pom.xml`, Gradle, `.java` | Maven or Gradle, then `jpackage` when it is installed |
| Kotlin | `.kt`, Gradle | Gradle, or `kotlinc` |
| C# | `.csproj`, `.cs` | `dotnet publish` |
| Ruby | `.rb`, `Gemfile` | App that runs with Ruby |
| PHP | `.php`, `composer.json` | App that runs with PHP |
| Perl | `.pl` | App that runs with Perl |
| Lua | `.lua` | App that runs with Lua |
| Shell | `.sh` | App that runs the script |
| Zig | `build.zig`, `.zig` | `zig build` |
| Dart | `pubspec.yaml`, `.dart` | `dart compile exe` |

Python apps need `py2app` (`python3 -m pip install py2app`). Python tools need PyInstaller (`python3 -m pip install pyinstaller`). Other languages need their normal compiler. The Toolchains list in the window shows what is installed.

Interpreted programs keep a launcher that calls the runtime on the machine that opens them. Rust, C, C++, Go, Zig, and `bun --compile` produce a native executable. `jpackage` includes a Java runtime when it is available.

## Command line

The same engine is available as `packager`:

```bash
cargo build --release --manifest-path rust/Cargo.toml
./rust/target/release/packager languages
./rust/target/release/packager detect path/to/project
./rust/target/release/packager build --source path/to/project --output ~/Desktop --name Hello --mode app
```

`--mode tool` writes a terminal program. `--language` overrides detection. Pass `--icon` with a `.icns` or `.png` file for an app icon.

## Tests

```bash
PACKAGER_TMP=.tmp cargo test --manifest-path rust/Cargo.toml
```

The tests detect projects and package small C, Rust, JavaScript, and shell programs.
