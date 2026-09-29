// Native references belong to the engine's fixed worker thread. No
// NSApplication is created and no main-thread event loop is taken over by the
// library.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <Carbon/Carbon.h>
#import <ImageIO/ImageIO.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct {
  atomic_bool stopped;
} DWCancel;
void *dw_cancel_new(void) {
  DWCancel *c = calloc(1, sizeof(DWCancel));
  if (c)
    atomic_init(&c->stopped, false);
  return c;
}
void dw_cancel_signal(void *p) {
  atomic_store_explicit(&((DWCancel *)p)->stopped, true, memory_order_release);
}
static BOOL cancelled(DWCancel *c) {
  return c && atomic_load_explicit(&c->stopped, memory_order_acquire);
}
@interface DWContext : NSObject
@property NSMutableArray *elements;
@property NSMutableArray *meta;
@property NSArray *lastDisplays;
@property NSUInteger topology;
@end
@implementation DWContext
@end
static NSDictionary *known(id v) {
  return
      @{@"Status" : @"known", @"Value" : v ?: @"", @"Source" : @"ui_content"};
}
static NSDictionary *unknown(void) { return @{@"Status" : @"unknown"}; }
static NSDictionary *err(NSString *code) {
  return @{
    @"Fault" :
        @{@"Code" : code, @"Message" : code, @"RetryClass" : @"reobserve"}
  };
}
static id attr(AXUIElementRef e, CFStringRef k) {
  CFTypeRef v = NULL;
  if (AXUIElementCopyAttributeValue(e, k, &v) != kAXErrorSuccess)
    return nil;
  return CFBridgingRelease(v);
}
static BOOL alive(DWContext *c, NSString *k);
static NSString *key(DWContext *c, AXUIElementRef e, NSString *app,
                     NSString *win, NSString *parent) {
  if (!e)
    return @"";
  for (NSUInteger i = 0; i < c.elements.count; i++) {
    NSString *existing =
        [NSString stringWithFormat:@"%lu", (unsigned long)i + 1];
    if (CFEqual((__bridge CFTypeRef)c.elements[i], e) && alive(c, existing)) {
      NSMutableDictionary *meta = [c.meta[i] mutableCopy];
      if (app)
        meta[@"App"] = app;
      if (win)
        meta[@"Window"] = win;
      if (parent)
        meta[@"Parent"] = parent;
      c.meta[i] = meta;
      return existing;
    }
  }
  if (c.elements.count >= 10000)
    return @"";
  [c.elements addObject:(__bridge id)e];
  pid_t pid = 0;
  AXUIElementGetPid(e, &pid);
  NSRunningApplication *ra =
      [NSRunningApplication runningApplicationWithProcessIdentifier:pid];
  [c.meta addObject:@{
    @"App" : app ?: @"",
    @"Window" : win ?: @"",
    @"Parent" : parent ?: @"",
    @"PID" : @(pid),
    @"Launch" : ra.launchDate ?: [NSDate distantPast]
  }];
  return [NSString stringWithFormat:@"%lu", (unsigned long)c.elements.count];
}
static AXUIElementRef element(DWContext *c, NSString *k) {
  NSInteger i = k.integerValue - 1;
  return i >= 0 && i < (NSInteger)c.elements.count
             ? (__bridge AXUIElementRef)c.elements[i]
             : NULL;
}
static BOOL alive(DWContext *c, NSString *k) {
  NSInteger i = k.integerValue - 1;
  if (i < 0 || i >= (NSInteger)c.meta.count)
    return NO;
  NSDictionary *m = c.meta[i];
  NSRunningApplication *ra = [NSRunningApplication
      runningApplicationWithProcessIdentifier:[m[@"PID"] intValue]];
  return ra && !ra.terminated &&
         [m[@"Launch"] isEqual:ra.launchDate ?: [NSDate distantPast]];
}
static NSArray *displays(DWContext *c) {
  CGDirectDisplayID ids[32];
  uint32_t n = 0;
  CGGetActiveDisplayList(32, ids, &n);
  NSMutableArray *a = [NSMutableArray array];
  for (uint32_t i = 0; i < n; i++) {
    CGRect b = CGDisplayBounds(ids[i]);
    [a addObject:@{
      @"Ref" : [NSString stringWithFormat:@"%u", ids[i]],
      @"Frame" : @"desktop",
      @"Bounds" : @{
        @"X" : @(b.origin.x),
        @"Y" : @(b.origin.y),
        @"Width" : @(b.size.width),
        @"Height" : @(b.size.height)
      },
      @"Unit" : @"quartz_point",
      @"ScaleX" : @(CGDisplayPixelsWide(ids[i]) / b.size.width),
      @"ScaleY" : @(CGDisplayPixelsHigh(ids[i]) / b.size.height),
      @"RotationDegrees" : @(CGDisplayRotation(ids[i]))
    }];
  }
  if (![a isEqual:c.lastDisplays]) {
    c.topology++;
    c.lastDisplays = [a copy];
  }
  return a;
}
static NSArray *permissions(void) {
  return @[
    @{
      @"Name" : @"accessibility",
      @"State" : AXIsProcessTrusted() ? @"granted" : @"not_requested"
    },
    @{
      @"Name" : @"input",
      @"State" : CGPreflightPostEventAccess() ? @"granted" : @"not_requested"
    },
    @{
      @"Name" : @"screen_capture",
      @"State" : CGPreflightScreenCaptureAccess() ? @"granted"
                                                  : @"not_requested"
    },
    @{@"Name" : @"user_input_observation", @"State" : @"not_requested"}
  ];
}
static NSString *roleName(NSString *r) {
  NSDictionary *m = @{
    @"AXApplication" : @"application",
    @"AXWindow" : @"window",
    @"AXButton" : @"button",
    @"AXTextField" : @"text_field",
    @"AXTextArea" : @"text_field",
    @"AXStaticText" : @"text",
    @"AXCheckBox" : @"checkbox",
    @"AXMenu" : @"menu",
    @"AXMenuItem" : @"menu_item",
    @"AXList" : @"list",
    @"AXRow" : @"list_item",
    @"AXTabGroup" : @"tab",
    @"AXGroup" : @"container",
    @"AXScrollArea" : @"container"
  };
  return m[r] ?: @"unknown";
}
static NSDictionary *node(DWContext *c, NSString *k) {
  AXUIElementRef e = element(c, k);
  if (!e || !alive(c, k))
    return nil;
  NSString *role = attr(e, kAXRoleAttribute);
  if (![role isKindOfClass:NSString.class])
    return nil;
  NSDictionary *meta = c.meta[k.integerValue - 1];
  NSString *kind = [role isEqual:@"AXApplication"] ? @"application"
                   : [role isEqual:@"AXWindow"]    ? @"window"
                                                   : @"ui";
  NSString *title = attr(e, kAXTitleAttribute);
  if (![title isKindOfClass:NSString.class] || !title.length)
    title = attr(e, kAXDescriptionAttribute);
  NSString *sub = attr(e, kAXSubroleAttribute);
  BOOL protected = [sub isEqual:@"AXSecureTextField"];
  NSMutableDictionary *states = [NSMutableDictionary dictionary];
  NSDictionary *stateAttrs = @{
    @"enabled" : (__bridge NSString *)kAXEnabledAttribute,
    @"focused" : (__bridge NSString *)kAXFocusedAttribute,
    @"selected" : (__bridge NSString *)kAXSelectedAttribute,
    @"expanded" : (__bridge NSString *)kAXExpandedAttribute
  };
  for (NSString *name in stateAttrs) {
    id v = attr(e, (__bridge CFStringRef)stateAttrs[name]);
    states[name] =
        [v isKindOfClass:NSNumber.class] ? known(@([v boolValue])) : unknown();
  };
  states[@"protected"] = known(@(protected));
  Boolean writable = false;
  AXError writableError =
      AXUIElementIsAttributeSettable(e, kAXValueAttribute, &writable);
  states[@"read_only"] =
      writableError == kAXErrorSuccess ? known(@((BOOL)!writable)) : unknown();
  id value = protected ? nil : attr(e, kAXValueAttribute);
  NSString *valueText = [value isKindOfClass:NSString.class] ? value
                        : [value isKindOfClass:NSNumber.class]
                            ? [value stringValue]
                            : nil;
  id pos = attr(e, kAXPositionAttribute), sz = attr(e, kAXSizeAttribute);
  CGPoint pt;
  CGSize size;
  NSDictionary *bounds = unknown();
  if (pos && sz && CFGetTypeID((__bridge CFTypeRef)pos) == AXValueGetTypeID() &&
      CFGetTypeID((__bridge CFTypeRef)sz) == AXValueGetTypeID() &&
      AXValueGetValue((__bridge AXValueRef)pos, kAXValueCGPointType, &pt) &&
      AXValueGetValue((__bridge AXValueRef)sz, kAXValueCGSizeType, &size)) {
    bounds = known(@{
      @"Frame" : @"desktop",
      @"Topology" : @(c.topology),
      @"Rect" : @{
        @"X" : @(pt.x),
        @"Y" : @(pt.y),
        @"Width" : @(size.width),
        @"Height" : @(size.height)
      }
    });
  }
  CFArrayRef rawActions = NULL;
  AXUIElementCopyActionNames(e, &rawActions);
  NSArray *actions = CFBridgingRelease(rawActions);
  Boolean focusable = false;
  AXUIElementIsAttributeSettable(e, kAXFocusedAttribute, &focusable);
  BOOL enabled = ![states[@"enabled"][@"Value"] isEqual:@NO];
  NSMutableArray *caps = [NSMutableArray array];
  NSDictionary *supported = @{
    @"focus" : @([kind isEqual:@"window"] || focusable),
    @"invoke" : @([actions containsObject:(__bridge NSString *)kAXPressAction]),
    @"set_value" : @(writable && !protected)
  };
  for (NSString *op in supported) {
    BOOL yes = [supported[op] boolValue];
    [caps addObject:@{
      @"Name" : op,
      @"Support" : yes ? @"supported" : @"unsupported",
      @"Availability" : yes && enabled ? @"available" : @"blocked"
    }];
  }
  [caps sortUsingComparator:^NSComparisonResult(NSDictionary *a,
                                                NSDictionary *b) {
    return [a[@"Name"] compare:b[@"Name"]];
  }];
  NSString *app = meta[@"App"], *win = meta[@"Window"];
  NSString *parent = meta[@"Parent"];
  if ([kind isEqual:@"ui"]) {
    id actualWindow = attr(e, kAXWindowAttribute);
    win = actualWindow
              ? key(c, (__bridge AXUIElementRef)actualWindow, app, nil, app)
              : @"";
    id actualParent = attr(e, kAXParentAttribute);
    parent = actualParent
                 ? key(c, (__bridge AXUIElementRef)actualParent, app, win, nil)
                 : @"";
  }
  if ([kind isEqual:@"application"])
    app = k;
  if ([kind isEqual:@"window"])
    win = k;
  return @{
    @"Key" : k,
    @"App" : app,
    @"Window" : win,
    @"Parent" : parent,
    @"Object" : @{
      @"Kind" : kind,
      @"Role" : roleName(role),
      @"Name" : [title isKindOfClass:NSString.class] ? known(title) : unknown(),
      @"ValuePreview" : protected
          ? @{@"Status" : @"redacted"}
          : (valueText ? known([valueText
                             substringToIndex:MIN(valueText.length, 384)])
                       : unknown()),
      @"States" : states,
      @"Bounds" : bounds,
      @"Capabilities" : caps,
      @"Lifecycle" : @"live"
    }
  };
}
static NSDictionary *seat(DWContext *c) {
  AXUIElementRef sys = AXUIElementCreateSystemWide();
  id app = attr(sys, kAXFocusedApplicationAttribute),
     focused = attr(sys, kAXFocusedUIElementAttribute);
  id win =
      app ? attr((__bridge AXUIElementRef)app, kAXFocusedWindowAttribute) : nil;
  NSString *ak =
      app ? key(c, (__bridge AXUIElementRef)app, nil, nil, nil) : @"";
  NSString *wk = win ? key(c, (__bridge AXUIElementRef)win, ak, nil, ak) : @"";
  NSString *fk =
      focused ? key(c, (__bridge AXUIElementRef)focused, ak, wk, wk) : @"";
  CFRelease(sys);
  CGEventRef ev = CGEventCreate(NULL);
  CGPoint p = ev ? CGEventGetLocation(ev) : CGPointZero;
  if (ev)
    CFRelease(ev);
  return @{
    @"Foreground" : wk,
    @"Focused" : fk,
    @"Pointer" : known(@{
      @"Frame" : @"desktop",
      @"Topology" : @(c.topology),
      @"X" : @(p.x),
      @"Y" : @(p.y)
    }),
    @"Health" : @"ready",
    @"Intervention" : @"best_effort"
  };
}
static NSDictionary *query(DWContext *c, NSDictionary *q, DWCancel *cancel) {
  if (!AXIsProcessTrusted())
    return err(@"permission_denied");
  NSMutableArray *queue = [NSMutableArray array];
  BOOL complete = YES;
  NSInteger depth = [q[@"Depth"] integerValue],
            max = [q[@"MaxNodes"] integerValue];
  BOOL summary = [q[@"Summary"] boolValue], detail = [q[@"Detail"] boolValue];
  if ([q[@"Desktop"] boolValue]) {
    for (NSRunningApplication *app in NSWorkspace.sharedWorkspace
             .runningApplications) {
      if (app.terminated ||
          app.activationPolicy == NSApplicationActivationPolicyProhibited)
        continue;
      AXUIElementRef e = AXUIElementCreateApplication(app.processIdentifier);
      AXUIElementSetMessagingTimeout(e, 0.25);
      NSString *k = key(c, e, nil, nil, nil);
      CFRelease(e);
      if (k.length)
        [queue addObject:@[ k, @0 ]];
      else
        complete = NO;
    }
  } else
    for (NSString *k in q[@"Roots"])
      [queue addObject:@[ k, @0 ]];
  NSMutableArray *nodes = [NSMutableArray array];
  NSMutableSet *seen = [NSMutableSet set];
  NSInteger visited = 0;
  while (queue.count) {
    if (cancelled(cancel) || visited >= max) {
      complete = NO;
      break;
    };
    NSArray *item = queue[0];
    [queue removeObjectAtIndex:0];
    NSString *k = item[0];
    if ([seen containsObject:k])
      continue;
    [seen addObject:k];
    visited++;
    NSDictionary *n = node(c, k);
    if (!n) {
      complete = NO;
      continue;
    };
    NSString *kind = n[@"Object"][@"Kind"];
    if (!summary || ![kind isEqual:@"ui"])
      [nodes addObject:n];
    NSInteger d = [item[1] integerValue];
    if (detail || d >= depth || (summary && ![kind isEqual:@"application"]))
      continue;
    AXUIElementRef e = element(c, k);
    CFStringRef field = summary ? kAXWindowsAttribute : kAXChildrenAttribute;
    CFIndex count = 0;
    AXError rc = AXUIElementGetAttributeValueCount(e, field, &count);
    if (rc == kAXErrorAttributeUnsupported)
      continue;
    if (rc != kAXErrorSuccess) {
      complete = NO;
      continue;
    };
    CFIndex take = MIN(count, MAX(0, max - visited - (NSInteger)queue.count));
    if (take < count)
      complete = NO;
    if (take <= 0)
      continue;
    CFArrayRef raw = NULL;
    rc = AXUIElementCopyAttributeValues(e, field, 0, take, &raw);
    if (rc != kAXErrorSuccess) {
      complete = NO;
      continue;
    };
    NSArray *children = CFBridgingRelease(raw);
    for (id child in children) {
      if (CFGetTypeID((__bridge CFTypeRef)child) != AXUIElementGetTypeID())
        continue;
      NSString *ck =
          key(c, (__bridge AXUIElementRef)child, n[@"App"], n[@"Window"], k);
      if (ck.length)
        [queue addObject:@[ ck, @(d + 1) ]];
      else
        complete = NO;
    }
  }
  return @{
    @"Result" : @{
      @"Nodes" : nodes,
      @"Complete" : @(complete),
      @"Visited" : @(visited),
      @"Seat" : seat(c),
      @"Unavailable" : complete ? @[] : @[ @"ax_partial" ]
    }
  };
}
static NSDictionary *outcome(NSString *d, NSString *f) {
  NSMutableDictionary *r = [@{@"Delivery" : d} mutableCopy];
  if ([d isEqual:@"unknown"])
    r[@"Unsafe"] = @YES;
  if (f)
    r[@"Fault"] =
        @{@"Code" : f, @"Message" : f, @"RetryClass" : @"never_automatically"};
  return @{@"Result" : r};
}
static BOOL postMouse(CGEventType type, CGPoint p, CGMouseButton b, int count) {
  CGEventRef e = CGEventCreateMouseEvent(NULL, type, p, b);
  if (e) {
    CGEventSetIntegerValueField(e, kCGMouseEventClickState, count);
    CGEventPost(kCGHIDEventTap, e);
    CFRelease(e);
    return YES;
  }
  return NO;
}
static CGKeyCode keycode(NSString *s) {
  NSDictionary *m = @{
    @"Enter" : @36,
    @"Tab" : @48,
    @"Space" : @49,
    @"Escape" : @53,
    @"Backspace" : @51,
    @"Delete" : @117,
    @"Left" : @123,
    @"Right" : @124,
    @"Up" : @126,
    @"Down" : @125,
    @"Home" : @115,
    @"End" : @119,
    @"PageUp" : @116,
    @"PageDown" : @121,
    @"A" : @0,
    @"S" : @1,
    @"D" : @2,
    @"F" : @3,
    @"H" : @4,
    @"G" : @5,
    @"Z" : @6,
    @"X" : @7,
    @"C" : @8,
    @"V" : @9,
    @"B" : @11,
    @"Q" : @12,
    @"W" : @13,
    @"E" : @14,
    @"R" : @15,
    @"Y" : @16,
    @"T" : @17,
    @"1" : @18,
    @"2" : @19,
    @"3" : @20,
    @"4" : @21,
    @"6" : @22,
    @"5" : @23,
    @"9" : @25,
    @"7" : @26,
    @"8" : @28,
    @"0" : @29,
    @"O" : @31,
    @"U" : @32,
    @"I" : @34,
    @"P" : @35,
    @"L" : @37,
    @"J" : @38,
    @"K" : @40,
    @"N" : @45,
    @"M" : @46
  };
  return m[s] ? [m[s] unsignedShortValue] : UINT16_MAX;
}
static NSDictionary *perform(DWContext *c, NSDictionary *o, DWCancel *cancel) {
  if (cancelled(cancel))
    return outcome(@"none", @"cancelled");
  NSDictionary *s = o[@"Step"];
  NSString *op = s[@"Op"], *k = o[@"Key"];
  AXUIElementRef e = element(c, k);
  if (k.length && (!e || !alive(c, k)))
    return outcome(@"none", @"ref_gone");
  if (!CGPreflightPostEventAccess())
    return outcome(@"none", @"permission_denied");
  if ([op isEqual:@"focus"] || [op isEqual:@"invoke"] ||
      [op isEqual:@"set_value"]) {
    AXError rc = kAXErrorFailure;
    if ([op isEqual:@"invoke"])
      rc = AXUIElementPerformAction(e, kAXPressAction);
    if ([op isEqual:@"set_value"])
      rc = AXUIElementSetAttributeValue(
          e, kAXValueAttribute, (__bridge CFTypeRef)s[@"SetValue"][@"Text"]);
    if ([op isEqual:@"focus"]) {
      NSString *role = attr(e, kAXRoleAttribute);
      if ([role isEqual:@"AXWindow"]) {
        pid_t pid = 0;
        AXUIElementGetPid(e, &pid);
        [[NSRunningApplication runningApplicationWithProcessIdentifier:pid]
            activateWithOptions:0];
        rc = AXUIElementPerformAction(e, kAXRaiseAction);
      } else
        rc = AXUIElementSetAttributeValue(e, kAXFocusedAttribute,
                                          kCFBooleanTrue);
    }
    if (rc == kAXErrorSuccess)
      return outcome(@"complete", nil);
    if (rc == kAXErrorCannotComplete || rc == kAXErrorFailure)
      return outcome(@"unknown", @"native_timeout");
    return outcome(@"none", @"capability_unavailable");
  }
  // Reject interference from held physical modifiers/buttons instead of
  // resetting them.
  if (CGEventSourceFlagsState(kCGEventSourceStateCombinedSessionState) &
      (kCGEventFlagMaskShift | kCGEventFlagMaskControl |
       kCGEventFlagMaskAlternate | kCGEventFlagMaskCommand))
    return outcome(@"none", @"user_interrupted");
  for (int b = 0; b < 3; b++)
    if (CGEventSourceButtonState(kCGEventSourceStateCombinedSessionState, b))
      return outcome(@"none", @"user_interrupted");
  NSDictionary *p = o[@"Point"];
  if (![p isKindOfClass:NSDictionary.class])
    p = nil;
  CGPoint pt = CGPointMake([p[@"X"] doubleValue], [p[@"Y"] doubleValue]);
  if ([op isEqual:@"pointer.move"]) {
    if (!postMouse(kCGEventMouseMoved, pt, kCGMouseButtonLeft, 0))
      return outcome(@"none", @"input_rejected");
  } else if ([op isEqual:@"pointer.click"]) {
    NSString *b = s[@"Click"][@"Button"];
    CGMouseButton button = [b isEqual:@"right"]    ? kCGMouseButtonRight
                           : [b isEqual:@"middle"] ? kCGMouseButtonCenter
                                                   : kCGMouseButtonLeft;
    CGEventType down = button == 0   ? kCGEventLeftMouseDown
                       : button == 1 ? kCGEventRightMouseDown
                                     : kCGEventOtherMouseDown;
    CGEventType up = button == 0   ? kCGEventLeftMouseUp
                     : button == 1 ? kCGEventRightMouseUp
                                   : kCGEventOtherMouseUp;
    for (int i = 1; i <= [s[@"Click"][@"Count"] intValue]; i++) {
      if (!postMouse(down, pt, button, i))
        return outcome(i == 1 ? @"none" : @"partial", @"input_rejected");
      if (!postMouse(up, pt, button, i)) {
        BOOL cleaned = postMouse(up, pt, button, i);
        return outcome(cleaned ? @"partial" : @"unknown", @"input_rejected");
      }
    }
  } else if ([op isEqual:@"pointer.drag"]) {
    NSDictionary *t = o[@"To"];
    CGPoint to = CGPointMake([t[@"X"] doubleValue], [t[@"Y"] doubleValue]);
    if (!postMouse(kCGEventLeftMouseDown, pt, 0, 1))
      return outcome(@"none", @"input_rejected");
    BOOL failed = NO;
    long long nanos = [s[@"Drag"][@"Duration"] longLongValue];
    int steps = MAX(1, MIN(120, (int)(nanos / 16666667)));
    CGPoint last = pt;
    for (int i = 1; i <= steps; i++) {
      if (cancelled(cancel))
        break;
      CGPoint pos = CGPointMake(pt.x + (to.x - pt.x) * i / steps,
                                pt.y + (to.y - pt.y) * i / steps);
      if (!postMouse(kCGEventLeftMouseDragged, pos, 0, 1)) {
        failed = YES;
        break;
      }
      last = pos;
      if (nanos > 0)
        usleep((useconds_t)(nanos / steps / 1000));
    }
    if (!postMouse(kCGEventLeftMouseUp, last, 0, 1)) {
      BOOL cleaned = postMouse(kCGEventLeftMouseUp, last, 0, 1);
      return outcome(cleaned ? @"partial" : @"unknown", @"input_rejected");
    }
    if (failed)
      return outcome(@"partial", @"input_rejected");
    if (cancelled(cancel))
      return outcome(@"partial", @"cancelled");
  } else if ([op isEqual:@"pointer.scroll"]) {
    if (!postMouse(kCGEventMouseMoved, pt, 0, 0))
      return outcome(@"none", @"input_rejected");
    CGEventRef ev = CGEventCreateScrollWheelEvent(
        NULL, kCGScrollEventUnitLine, 2, -[s[@"Scroll"][@"DY"] intValue],
        -[s[@"Scroll"][@"DX"] intValue]);
    if (!ev)
      return outcome(@"partial", @"input_rejected");
    CGEventPost(kCGHIDEventTap, ev);
    CFRelease(ev);
  } else if ([op isEqual:@"keyboard.type_text"]) {
    NSString *text = s[@"TypeText"][@"Text"];
    for (NSUInteger i = 0; i < text.length;) {
      if (cancelled(cancel))
        return outcome(i ? @"partial" : @"none", @"cancelled");
      NSUInteger count = 1;
      unichar first = [text characterAtIndex:i];
      if (CFStringIsSurrogateHighCharacter(first) && i + 1 < text.length)
        count = 2;
      UniChar chars[2];
      [text getCharacters:chars range:NSMakeRange(i, count)];
      CGEventRef down = CGEventCreateKeyboardEvent(NULL, 0, true),
                 up = CGEventCreateKeyboardEvent(NULL, 0, false);
      if (!down || !up) {
        if (down)
          CFRelease(down);
        if (up)
          CFRelease(up);
        return outcome(i ? @"partial" : @"none", @"input_rejected");
      }
      CGEventKeyboardSetUnicodeString(down, count, chars);
      CGEventKeyboardSetUnicodeString(up, count, chars);
      CGEventPost(kCGHIDEventTap, down);
      CGEventPost(kCGHIDEventTap, up);
      CFRelease(down);
      CFRelease(up);
      i += count;
    }
  } else if ([op isEqual:@"keyboard.press"]) {
    CGKeyCode code = keycode(s[@"Press"][@"Key"]);
    if (code == UINT16_MAX)
      return outcome(@"none", @"capability_unavailable");
    NSArray *mods = s[@"Press"][@"Modifiers"];
    if (![mods isKindOfClass:NSArray.class])
      mods = @[];
    NSDictionary *codes = @{
      @"primary" : @55,
      @"meta" : @55,
      @"control" : @59,
      @"alt" : @58,
      @"shift" : @56
    };
    NSDictionary *masks = @{
      @"primary" : @(kCGEventFlagMaskCommand),
      @"meta" : @(kCGEventFlagMaskCommand),
      @"control" : @(kCGEventFlagMaskControl),
      @"alt" : @(kCGEventFlagMaskAlternate),
      @"shift" : @(kCGEventFlagMaskShift)
    };
    CGEventRef events[12] = {0};
    NSUInteger count = 0;
    CGEventFlags flags = 0;
    BOOL valid = YES;
    for (NSString *m in mods) {
      flags |= [masks[m] unsignedLongLongValue];
      CGEventRef ev =
          CGEventCreateKeyboardEvent(NULL, [codes[m] unsignedShortValue], true);
      if (ev)
        CGEventSetFlags(ev, flags);
      else
        valid = NO;
      events[count++] = ev;
    }
    for (int down = 1; down >= 0; down--) {
      CGEventRef ev = CGEventCreateKeyboardEvent(NULL, code, down);
      if (ev)
        CGEventSetFlags(ev, flags);
      else
        valid = NO;
      events[count++] = ev;
    }
    for (NSString *m in [mods reverseObjectEnumerator]) {
      flags &= ~[masks[m] unsignedLongLongValue];
      CGEventRef ev = CGEventCreateKeyboardEvent(
          NULL, [codes[m] unsignedShortValue], false);
      if (ev)
        CGEventSetFlags(ev, flags);
      else
        valid = NO;
      events[count++] = ev;
    }
    BOOL stopped = cancelled(cancel);
    if (valid && !stopped) {
      for (NSUInteger i = 0; i < count; i++)
        CGEventPost(kCGHIDEventTap, events[i]);
    }
    for (NSUInteger i = 0; i < count; i++)
      if (events[i])
        CFRelease(events[i]);
    if (!valid || stopped)
      return outcome(@"none", stopped ? @"cancelled" : @"input_rejected");

  }

  else
    return outcome(@"none", @"capability_unavailable");
  return outcome(@"complete", nil);
}
static NSDictionary *capture(DWContext *c, NSDictionary *r) {
  if (![r[@"Kind"] isEqual:@"visible_region"])
    return err(@"capability_unavailable");
  if (!CGPreflightScreenCaptureAccess())
    return err(@"permission_denied");
  if (@available(macOS 14.0, *)) {
    __block SCShareableContent *content = nil;
    __block NSError *error = nil;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    [SCShareableContent
        getShareableContentExcludingDesktopWindows:NO
                               onScreenWindowsOnly:YES
                                 completionHandler:^(SCShareableContent *v,
                                                     NSError *e) {
                                   content = v;
                                   error = e;
                                   dispatch_semaphore_signal(sem);
                                 }];
    dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
    if (error || !content)
      return err(@"provider_unavailable");
    NSMutableArray *images = [NSMutableArray array];
    NSDictionary *region = r[@"Region"];
    if ((id)region == NSNull.null)
      region = nil;
    for (SCDisplay *d in content.displays) {
      CGRect frame = CGDisplayBounds(d.displayID);
      CGRect clipped = frame;
      if (region) {
        NSDictionary *b = region[@"Rect"];
        clipped = CGRectIntersection(
            frame,
            CGRectMake([b[@"X"] doubleValue], [b[@"Y"] doubleValue],
                       [b[@"Width"] doubleValue], [b[@"Height"] doubleValue]));
      }
      if (CGRectIsEmpty(clipped) || CGRectIsNull(clipped))
        continue;
      SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:d
                                                        excludingWindows:@[]];
      SCStreamConfiguration *config = [SCStreamConfiguration new];
      CGFloat scale =
          MIN(CGDisplayPixelsWide(d.displayID) / frame.size.width,
              MIN([r[@"MaxPixelWidth"] doubleValue] / clipped.size.width,
                  [r[@"MaxPixelHeight"] doubleValue] / clipped.size.height));
      config.width = MAX(1, (size_t)(clipped.size.width * scale));
      config.height = MAX(1, (size_t)(clipped.size.height * scale));
      config.sourceRect = CGRectMake(clipped.origin.x - frame.origin.x,
                                     clipped.origin.y - frame.origin.y,
                                     clipped.size.width, clipped.size.height);
      config.showsCursor = [r[@"IncludeCursor"] boolValue];
      __block CGImageRef img = NULL;
      error = nil;
      [SCScreenshotManager
          captureImageWithFilter:filter
                   configuration:config
               completionHandler:^(CGImageRef image, NSError *e) {
                 if (image)
                   img = CGImageRetain(image);
                 error = e;
                 dispatch_semaphore_signal(sem);
               }];
      dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
      if (error || !img)
        return err(@"provider_unavailable");
      NSMutableData *data = [NSMutableData data];
      CGImageDestinationRef dest = CGImageDestinationCreateWithData(
          (__bridge CFMutableDataRef)data,
          (__bridge CFStringRef)UTTypePNG.identifier, 1, NULL);
      if (!dest) {
        CGImageRelease(img);
        return err(@"provider_unavailable");
      }
      CGImageDestinationAddImage(dest, img, NULL);
      BOOL ok = CGImageDestinationFinalize(dest);
      CFRelease(dest);
      CGImageRelease(img);
      if (!ok)
        return err(@"provider_unavailable");
      [images addObject:@{
        @"Bytes" : [data base64EncodedStringWithOptions:0],
        @"ContentType" : @"image/png",
        @"Width" : @(config.width),
        @"Height" : @(config.height),
        @"Bounds" : @{
          @"Frame" : @"desktop",
          @"Topology" : @(c.topology),
          @"Rect" : @{
            @"X" : @(clipped.origin.x),
            @"Y" : @(clipped.origin.y),
            @"Width" : @(clipped.size.width),
            @"Height" : @(clipped.size.height)
          }
        }
      }];
    }
    return @{@"Result" : images};
  }
  return err(@"platform_unsupported");
}
void *dw_open(void) {
  @autoreleasepool {
    DWContext *c = [DWContext new];
    c.elements = [NSMutableArray array];
    c.meta = [NSMutableArray array];
    c.topology = 0;
    displays(c);
    AXUIElementRef sys = AXUIElementCreateSystemWide();
    AXUIElementSetMessagingTimeout(sys, 0.25);
    CFRelease(sys);
    return (__bridge_retained void *)c;
  }
}
void dw_close(void *p) {
  @autoreleasepool {
    CFBridgingRelease(p);
  }
}
char *dw_call(void *p, const char *opstr, const char *json, void *cancel) {
  @autoreleasepool {
    @try {
      DWContext *c = (__bridge DWContext *)p;
      NSString *op = @(opstr);
      NSDictionary *r = [NSJSONSerialization
          JSONObjectWithData:[@(json) dataUsingEncoding:NSUTF8StringEncoding]
                     options:NSJSONReadingFragmentsAllowed
                       error:nil];
      NSDictionary *out = nil;
      if ([op isEqual:@"environment"]) {
        NSArray *ds = displays(c);
        out = @{
          @"Result" : @{
            @"Platform" : @"darwin",
            @"Topology" : @(c.topology),
            @"Displays" : ds,
            @"Permissions" : permissions(),
            @"Capabilities" : @[
              @{
                @"Name" : @"visible_region",
                @"Support" : @"supported",
                @"Availability" : CGPreflightScreenCaptureAccess()
                    ? @"available"
                    : @"blocked"
              },
              @{
                @"Name" : @"window_content",
                @"Support" : @"unsupported",
                @"Availability" : @"blocked"
              }
            ]
          }
        };
      } else if ([op isEqual:@"permissions"]) {
        for (NSString *n in r[@"Names"]) {
          if ([n isEqual:@"accessibility"]) {
            NSDictionary *o =
                @{(__bridge NSString *)kAXTrustedCheckOptionPrompt : @YES};
            AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)o);
          } else if ([n isEqual:@"screen_capture"])
            CGRequestScreenCaptureAccess();
          else if ([n isEqual:@"input"])
            CGRequestPostEventAccess();
        }
        out = @{@"Result" : permissions()};
      } else if ([op isEqual:@"query"])
        out = query(c, r, (DWCancel *)cancel);
      else if ([op isEqual:@"read"]) {
        NSDictionary *n = node(c, r[@"Key"]);
        out = n
                  ? @{@"Result" : @{@"Node" : n, @"Seat" : seat(c)}}
                  : err(alive(c, r[@"Key"]) ? @"provider_unavailable"
                                            : @"ref_gone");
      } else if ([op isEqual:@"text"]) {
        AXUIElementRef e = element(c, r[@"Key"]);
        if (!e || !alive(c, r[@"Key"]))
          out = err(@"ref_gone");
        else if ([attr(e, kAXSubroleAttribute) isEqual:@"AXSecureTextField"])
          out = @{
            @"Result" :
                @{@"Value" : @{@"Status" : @"redacted"}, @"Source" : @"value"}
          };
        else {
          NSString *source = @"value";
          id v = attr(e, kAXValueAttribute);
          if (![v isKindOfClass:NSString.class]) {
            v = attr(e, kAXTitleAttribute);
            source = @"label";
          }
          out = @{
            @"Result" : @{
              @"Value" : [v isKindOfClass:NSString.class] ? known(v)
                                                          : unknown(),
              @"Source" : source
            }
          };
        }
      } else if ([op isEqual:@"perform"])
        out = perform(c, r, (DWCancel *)cancel);
      else if ([op isEqual:@"hit"]) {
        AXUIElementRef sys = AXUIElementCreateSystemWide(), hit = NULL;
        NSDictionary *p = r[@"Point"];
        BOOL ok = AXUIElementCopyElementAtPosition(sys, [p[@"X"] floatValue],
                                                   [p[@"Y"] floatValue],
                                                   &hit) == kAXErrorSuccess;
        AXUIElementRef wanted = element(c, r[@"Key"]);
        ok = ok && wanted && CFEqual(hit, wanted);
        if (hit)
          CFRelease(hit);
        CFRelease(sys);
        out = @{@"Result" : @(ok)};
      } else if ([op isEqual:@"capture"])
        out = capture(c, r);
      else
        out = err(@"capability_unavailable");
      NSData *data =
          [NSJSONSerialization dataWithJSONObject:out
                                          options:NSJSONWritingFragmentsAllowed
                                            error:nil];
      return strdup([[NSString alloc] initWithData:data
                                          encoding:NSUTF8StringEncoding]
                            .UTF8String
                        ?: "{}");
    } @catch (NSException *exception) {
      return strdup("{\"Fault\":{\"Code\":\"provider_unavailable\",\"Message\":"
                    "\"native exception\",\"RetryClass\":\"reobserve\"}}");
    }
  }
}
