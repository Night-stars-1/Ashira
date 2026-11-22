import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  /// Height of the App's header rows (PanelHeader); the traffic lights sit centered in it.
  private let headerHeight: CGFloat = 48

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    // The App draws under a see-through title bar; only the traffic lights stay native.
    titleVisibility = .hidden
    titlebarAppearsTransparent = true
    styleMask.insert(.fullSizeContentView)

    // window_frame.dart leaves this much room at the left of the top-left header.
    FlutterMethodChannel(name: "ashira/window", binaryMessenger: flutterViewController.engine.binaryMessenger)
      .setMethodCallHandler { [weak self] call, result in
        guard call.method == "trafficLightsWidth", let self else { return result(FlutterMethodNotImplemented) }
        self.placeTrafficLights()
        result(Double(self.standardWindowButton(.zoomButton).map { $0.frame.maxX + 12 } ?? 0))
      }

    // AppKit puts the buttons back at their default spot when it lays the title bar out again.
    for name in [NSWindow.didResizeNotification, NSWindow.didExitFullScreenNotification, NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification] {
      NotificationCenter.default.addObserver(forName: name, object: self, queue: .main) { [weak self] _ in self?.placeTrafficLights() }
    }

    RegisterGeneratedPlugins(registry: flutterViewController)

    super.awakeFromNib()
    placeTrafficLights()
  }

  /// Centers the buttons in a header-high corner, as far from the left edge as from the top
  /// and bottom, and shrinks the title bar container to that corner: everywhere else in the
  /// top row clicks reach the App, not AppKit. Full screen keeps AppKit's own bar.
  private func placeTrafficLights() {
    guard !styleMask.contains(.fullScreen),
          let close = standardWindowButton(.closeButton),
          let minimize = standardWindowButton(.miniaturizeButton),
          let zoom = standardWindowButton(.zoomButton),
          let container = close.superview?.superview else { return }
    let gap = minimize.frame.minX - close.frame.maxX
    let margin = (headerHeight - close.frame.height) / 2
    var x = margin
    for button in [close, minimize, zoom] {
      button.setFrameOrigin(NSPoint(x: x, y: margin))
      x += button.frame.width + gap
    }
    container.frame = NSRect(x: 0, y: frame.height - headerHeight, width: x - gap + margin, height: headerHeight)
  }
}
