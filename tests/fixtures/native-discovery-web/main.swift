import AppKit
import Foundation
import WebKit

// Owned acceptance fixture. No external documents, URLs, browser profile or
// persistent WebKit storage are used. The output directory is a CLI argument.
guard CommandLine.arguments.count == 2 else {
    fatalError("Usage: DTWDiscoveryWeb <output-directory>")
}
let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
let fixtureApplication = NSApplication.shared
fixtureApplication.setActivationPolicy(.regular)

private let document = #"""
<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'none'; img-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'">
<title>DTW Discovery Browser</title>
<style>
body { margin: 0; padding: 20px; font: 16px system-ui; color: #151515; background: white; }
h1 { font-size: 22px; } #count { position: sticky; top: 0; background: white; padding: 12px; }
main { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; }
button { font: inherit; padding: 12px; min-height: 48px; }
</style></head><body>
<h1>DTW owned discovery fixture</h1><p id="count" aria-live="polite">Count: 0</p>
<main id="controls" aria-label="Discovery controls"></main>
<script>
let started = 0, clicks = 0;
function publish(phase, button = null) {
  window.webkit.messageHandlers.dtwFixture.postMessage({phase, actions_started: started, clicks, button});
}
for (let i = 0; i < 420; i++) {
  const button = document.createElement('button');
  button.type = 'button';
  button.textContent = i === 419 ? 'Discovery Tail' : 'Discovery Button ' + String(i).padStart(3, '0');
  button.addEventListener('click', () => {
    started++;
    publish('started', button.textContent);
    clicks++;
    document.getElementById('count').textContent = 'Count: ' + clicks;
    publish('completed', button.textContent);
  });
  document.getElementById('controls').appendChild(button);
}
publish('ready');
</script></body></html>
"""#

final class FixtureDelegate: NSObject, NSApplicationDelegate, WKScriptMessageHandler, WKNavigationDelegate {
    private var window: NSWindow!
    private var webView: WKWebView!
    private var timer: Timer?
    private var loaded = false
    private var documentReady = false
    private var started = 0
    private var clicks = 0
    private var lastButton: String?
    private var lastError: String?
    private var command = ""

    private func save() {
        let state: [String: Any] = [
            "pid": ProcessInfo.processInfo.processIdentifier,
            "ready": loaded && documentReady,
            "actions_started": started,
            "clicks": clicks,
            "last_button": lastButton as Any? ?? NSNull(),
            "error": lastError as Any? ?? NSNull(),
            "active": fixtureApplication.isActive,
            "command": command,
            "button_count": 420,
            "content_source": "embedded_html",
            "windows": window == nil ? [] : [["id": window.windowNumber, "title": window.title,
                                               "visible": window.isVisible]]
        ]
        do {
            try JSONSerialization.data(withJSONObject: state, options: [.sortedKeys])
                .write(to: output.appendingPathComponent("state.json"), options: .atomic)
        } catch {
            FileHandle.standardError.write(Data("Fixture state write failed: \(error)\n".utf8))
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.userContentController.add(self, name: "dtwFixture")
        webView = WKWebView(frame: .zero, configuration: configuration)
        webView.navigationDelegate = self
        window = NSWindow(contentRect: NSRect(x: 180, y: 160, width: 900, height: 640),
                          styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "DTW Discovery Browser"
        window.isReleasedWhenClosed = false
        window.contentView = webView
        window.makeKeyAndOrderFront(nil)
        fixtureApplication.activate(ignoringOtherApps: true)
        save()
        webView.loadHTMLString(document, baseURL: nil)
        timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            guard let self,
                  let next = try? String(contentsOf: output.appendingPathComponent("command"), encoding: .utf8),
                  next != self.command else { return }
            self.command = next
            switch next.trimmingCharacters(in: .whitespacesAndNewlines) {
            case "quit": fixtureApplication.terminate(nil)
            case "snapshot": self.save()
            default: self.save()
            }
        }
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard message.name == "dtwFixture", message.frameInfo.isMainFrame,
              let body = message.body as? [String: Any],
              let phase = body["phase"] as? String,
              ["ready", "started", "completed"].contains(phase),
              let actualStarted = body["actions_started"] as? Int,
              let actualClicks = body["clicks"] as? Int,
              actualStarted >= 0, actualClicks >= 0, actualClicks <= actualStarted else { return }
        started = actualStarted
        clicks = actualClicks
        if let button = body["button"] as? String { lastButton = button }
        if phase == "ready" { documentReady = true }
        save()
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        loaded = true
        save()
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        lastError = String(describing: error)
        save()
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        decisionHandler(navigationAction.request.url?.absoluteString == "about:blank" ? .allow : .cancel)
    }

    func applicationDidBecomeActive(_ notification: Notification) { save() }
    func applicationDidResignActive(_ notification: Notification) { save() }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationWillTerminate(_ notification: Notification) {
        timer?.invalidate()
        webView?.configuration.userContentController.removeScriptMessageHandler(forName: "dtwFixture")
    }
}

let delegate = FixtureDelegate()
fixtureApplication.delegate = delegate
fixtureApplication.run()
