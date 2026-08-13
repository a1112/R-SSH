import AppKit
import ApplicationServices
import Foundation

let trusted = AXIsProcessTrustedWithOptions([
    kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: false
] as CFDictionary)
guard trusted else {
    fputs("macOS Accessibility authorization is required\n", stderr)
    exit(3)
}

var arguments = Array(CommandLine.arguments.dropFirst())
var pid: pid_t?
if let marker = arguments.firstIndex(of: "--pid") {
    guard marker + 1 < arguments.count, let parsed = Int32(arguments[marker + 1]) else { exit(2) }
    pid = parsed
    arguments.removeSubrange(marker...(marker + 1))
}
if let pid, let app = NSRunningApplication(processIdentifier: pid) {
    app.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])
}
guard let action = arguments.first else { exit(2) }
arguments.removeFirst()

func post(_ event: CGEvent) { CGEventPost(.cghidEventTap, event) }
func typeText(_ value: String) {
    for scalar in value.utf16 {
        let event = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true)!
        var unit = scalar
        event.keyboardSetUnicodeString(stringLength: 1, unicodeString: &unit)
        post(event)
        post(CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false)!)
    }
}
func point(_ x: String, _ y: String) -> CGPoint {
    CGPoint(x: Double(x)!, y: Double(y)!)
}
func button(_ value: String) -> CGMouseButton {
    value == "right" ? .right : value == "middle" ? .center : .left
}

switch action {
case "focus": break
case "type": typeText(arguments.joined(separator: " "))
case "key":
    let keys: [String: CGKeyCode] = ["enter": 36, "tab": 48, "escape": 53, "backspace": 51, "left": 123, "right": 124, "down": 125, "up": 126]
    guard let key = keys[arguments[0].lowercased()] else { exit(2) }
    post(CGEvent(keyboardEventSource: nil, virtualKey: key, keyDown: true)!)
    post(CGEvent(keyboardEventSource: nil, virtualKey: key, keyDown: false)!)
case "click":
    let location = point(arguments[0], arguments[1]); let selected = button(arguments[2])
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: location, mouseButton: selected)!)
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: location, mouseButton: selected)!)
case "drag":
    let start = point(arguments[0], arguments[1]); let end = point(arguments[2], arguments[3]); let selected = button(arguments[4])
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: start, mouseButton: selected)!)
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseDragged, mouseCursorPosition: end, mouseButton: selected)!)
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: end, mouseButton: selected)!)
case "wheel":
    post(CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 2, wheel1: Int32(arguments[1])!, wheel2: Int32(arguments[0])!, wheel3: 0)!)
case "paste":
    NSPasteboard.general.clearContents(); NSPasteboard.general.setString(arguments.joined(separator: " "), forType: .string)
    let down = CGEvent(keyboardEventSource: nil, virtualKey: 9, keyDown: true)!; down.flags = .maskCommand; post(down)
    let up = CGEvent(keyboardEventSource: nil, virtualKey: 9, keyDown: false)!; up.flags = .maskCommand; post(up)
case "resize":
    guard let pid else { exit(2) }
    let app = AXUIElementCreateApplication(pid); var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &value) == .success,
          let windows = value as? [AXUIElement], let window = windows.first else { exit(4) }
    var size = CGSize(width: Double(arguments[0])!, height: Double(arguments[1])!)
    let sizeValue = AXValueCreate(.cgSize, &size)!
    guard AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, sizeValue) == .success else { exit(4) }
case "window":
    guard let pid, let running = NSRunningApplication(processIdentifier: pid) else { exit(4) }
    let app = AXUIElementCreateApplication(pid); var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &value) == .success,
          let windows = value as? [AXUIElement], let window = windows.first else { exit(4) }
    switch arguments[0] {
    case "minimize": AXUIElementSetAttributeValue(window, kAXMinimizedAttribute as CFString, kCFBooleanTrue)
    case "maximize": AXUIElementSetAttributeValue(window, kAXFullScreenAttribute as CFString, kCFBooleanTrue)
    case "restore":
        AXUIElementSetAttributeValue(window, kAXMinimizedAttribute as CFString, kCFBooleanFalse)
        AXUIElementSetAttributeValue(window, kAXFullScreenAttribute as CFString, kCFBooleanFalse)
        running.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])
    case "close": running.terminate()
    default: exit(2)
    }
default: exit(2)
}
