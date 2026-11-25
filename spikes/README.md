# Spikes

Experiments from 23 November 2026, on an M1 Max with macOS 27.0.1 and Xcode 27.0. Each one
answers a question the player's design depends on.

## Can a music source live in another app?

[`extensionkit/`](extensionkit) is a sandboxed host with an extension point, and two copies of
one extension: one inside the host's bundle, one inside a second app that stands in for a
source from another developer. The host finds both, starts each in its own process and talks to
it over XPC. The contract has the shape of Alpaca's source contract on Android: a version, a
request with an answer, and the audio as a pipe.

The host was signed with a development certificate; the second app had an ad hoc signature and
no team.

| Measure | Inside the host | In the other app |
| --- | --- | --- |
| Start and first answer | 49 ms | 359 ms |
| Request and answer, 256 bytes | p50 0.06 ms, p99 0.16 ms | p50 0.06 ms, p99 0.11 ms |
| Pipe, as fast as it goes | 8.4 GB/s | 8.8 GB/s |
| Pipe at real time, write to read, 430 chunks in 10 s | p50 0.03 ms, p99 0.5 ms, max 1.8 ms | p50 0.03 ms, p99 0.5 ms, max 3.3 ms |

16-bit stereo at 44.1 kHz is 0.18 MB/s. The first start of a freshly signed extension took
1.9 s. A later run had one chunk 12 ms late, so the player's buffer has to cover more than
the table's worst case.

What it showed:

- The build setting `EX_ENABLE_EXTENSION_POINT_GENERATION=YES` turns the host's
  `@Definition` into `Contents/Extensions/SpikeHost.appexpt`, with `_EXScopeRestriction` set
  to `none`, and the extension's `@AppExtensionPoint.Bind` into `EXExtensionPointIdentifier` in
  its `Info.plist`. The identifier is the host's bundle identifier, a dot and the point's name.
- macOS reports an extension from another app as unapproved, and the host cannot start it,
  until the person at the Mac switches it on. `EXAppExtensionBrowserViewController` is the
  system's list for that; the host shows it with `--browse`.
- Once switched on, it runs like the one inside the host.
- After the other app was built again in another folder, macOS reported its extension as
  unapproved again. The build had a new ad hoc signature and a new path; which of the two
  cost it the approval was not tested.

What it did not show:

- A host signed for the App Store. That takes a TestFlight build.
- An extension signed by a second developer team.
- Whether an update of a source signed with a Developer ID keeps its approval.
- An extension that presents its own settings screen in the host.

### Build and run

It needs Xcode and [XcodeGen](https://github.com/yonaskolb/XcodeGen).

```sh
cd spikes/extensionkit
xcodegen generate
xcodebuild -project ExtSpike.xcodeproj -alltargets -configuration Release \
  SYMROOT=$PWD/out OBJROOT=$PWD/obj build

out/Release/SpikeHost.app/Contents/MacOS/SpikeHost --browse   # switch ThirdPartySource on
out/Release/SpikeHost.app/Contents/MacOS/SpikeHost            # find, start and measure
```

The build signs everything ad hoc. To sign the host with a certificate, as in the run above:

```sh
codesign -f -s "<identity>" -o runtime --entitlements Ext/Ext.entitlements \
  out/Release/SpikeHost.app/Contents/Extensions/EmbeddedSource.appex
codesign -f -s "<identity>" -o runtime --entitlements Host/Host.entitlements \
  out/Release/SpikeHost.app
```

## How fast is the example app?

walkit's macOS example (`examples/macos`, release build) with its built-in skins and the demo
player running:

| Skin | CPU | Memory | Zoom on a Retina screen |
| --- | --- | --- | --- |
| Classic | 47% of one core | 68 MB | 1, so 2 device pixels per layout pixel |
| Modern | 99% | 69 MB | 2, so 4 |
| No skin shown | 4% | 32 MB | |

The window was 808 by 687 points, in front, on a 120 Hz screen.

Where the time goes:

- The engine asks for about 40 frames a second while a track plays: the visualizer steps every
  25 ms, and a classic main window moves with every step of its title, time and visualizer.
  A tick costs 5 to 35 microseconds.
- For each of those frames the example draws every window from nothing, at the screen's
  device pixels, and hands the layer a new picture, which is then converted to the screen's
  colors. Drawing alone, measured outside the app:

  | Device pixels per layout pixel | Classic, 275 by 348 | Modern, 300 by 292 |
  | --- | --- | --- |
  | 1 | 4.3 ms | 4.2 ms |
  | 2 | 6.7 ms | 7.3 ms |
  | 4 | 16.6 ms | 19.8 ms |

- With no skin the display link still calls the app 120 times a second, which is the 4%.

So a frame costs as much as it does because of where and how large it is drawn, and the
engine's 40 frames a second are few. `--screenshot` took 1.5 s, of which 1.3 s is the waiting
written into it, so the time to the first frame needs a measurement of its own. The app is
12 MB.

## Which licenses does the engine bring?

The example's Rust dependencies (`walkit`, `image`, `zip` and what they need) are MIT, Apache
2.0, BSD or Zlib. walkit itself is GPL 3.0 or later, with one author.
