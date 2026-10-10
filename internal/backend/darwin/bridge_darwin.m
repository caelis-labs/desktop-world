// Native references belong to the engine's fixed worker thread. No
// NSApplication is created and no main-thread event loop is taken over by the
// library.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <Carbon/Carbon.h>
#import <ImageIO/ImageIO.h>
#import <CoreImage/CoreImage.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#include <libproc.h>
#include <dlfcn.h>
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
@interface DWCaptureWindow : NSObject
@property SCWindow *window;
@property NSArray *identity;
@property NSString *app;
@property pid_t pid;
@property BOOL gone;
@end
@implementation DWCaptureWindow
@end
@interface DWContext : NSObject
@property NSMutableDictionary *captureWindows;
@property NSMutableArray *elements;
@property NSMutableArray *meta;
@property NSMutableDictionary *scans;
@property NSArray *lastDisplays;
@property NSUInteger topology;
@property NSMutableDictionary *inputSession;
@property NSDictionary *inputReport;
@property NSString *inputFault;
@property NSMutableDictionary *inputHeld;
@end
@implementation DWContext
@end
// A scan is retained only inside this helper process. Its opaque ID is never
// authority: the engine rechecks turn, scope and request parameters on resume.
@interface DWScan : NSObject
@property NSMutableArray *queue;
@property NSMutableSet *seen;
@property NSUInteger head, visited;
@property BOOL incomplete, limitHit;
@property NSDate *expires;
@end
@implementation DWScan
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
static NSDictionary *scanCapacityError(void) {
  return @{ @"Fault" : @{
    @"Code" : @"ax_scan_capacity",
    @"Message" : @"16 native scan cursors are active; resume one or wait for expiry",
    @"RetryClass" : @"never_automatically"
  }};
}
static AXUIElementRef element(DWContext *c, NSString *k);
static BOOL alive(DWContext *c, NSString *k);
// The public DTW runtime never dispatches an action to terminal applications.
// Resolve the process from the retained AX element immediately before native
// delivery, so changing a model-visible window title cannot bypass the rule.
static NSString *terminalTargetFailure(DWContext *c, NSDictionary *request) {
  NSDictionary *operation = request[@"Operation"] ?: request;
  for (NSString *field in @[@"Key", @"ToKey"]) {
    NSString *key = operation[field];
    if (![key isKindOfClass:NSString.class] || !key.length) continue;
    AXUIElementRef target = element(c, key);
    pid_t pid = 0;
    if (!target || !alive(c, key) || AXUIElementGetPid(target, &pid) != kAXErrorSuccess || pid <= 0)
      return @"target_identity_unknown";
    NSRunningApplication *app = [NSRunningApplication runningApplicationWithProcessIdentifier:pid];
    if (!app) return @"target_identity_unknown";
    NSString *bundle = app.bundleIdentifier.lowercaseString ?: @"";
    NSString *executable = app.executableURL.lastPathComponent.lowercaseString ?: @"";
    NSString *bundlePath = app.bundleURL.lastPathComponent.lowercaseString ?: @"";
    if (!bundle.length && !executable.length) return @"target_identity_unknown";
    NSSet *bundles = [NSSet setWithArray:@[@"com.apple.terminal", @"com.googlecode.iterm2",
      @"com.mitchellh.ghostty", @"dev.warp.warp-stable", @"dev.warp.warp",
      @"org.alacritty", @"net.kovidgoyal.kitty", @"com.github.wez.wezterm",
      @"co.zeit.hyper", @"com.raphaelamorim.rio", @"com.electron.tabby"]];
    NSSet *names = [NSSet setWithArray:@[@"terminal", @"iterm2", @"ghostty", @"warp",
      @"alacritty", @"kitty", @"wezterm-gui", @"hyper", @"rio", @"tabby"]];
    if ([bundles containsObject:bundle] || [names containsObject:executable] ||
        [names containsObject:[bundlePath stringByDeletingPathExtension]])
      return @"terminal_application_blocked";
  }
  return nil;
}
// Keep previews, full reads and predicates on the same AX scalar contract.
// Unsupported values stay unknown; labels are a separately identified source.
static NSString *scalarText(id value) {
  if ([value isKindOfClass:NSString.class]) return value;
  if ([value isKindOfClass:NSNumber.class]) return [value stringValue];
  return nil;
}
// Bound previews in UTF-16 without cutting a surrogate pair. Full scalar reads
// retain the entire value; preview-boundary hits must not prove full text.
static NSString *scalarPreview(NSString *value) {
  NSUInteger end = MIN(value.length, 384);
  if (end < value.length && end > 0 &&
      CFStringIsSurrogateHighCharacter([value characterAtIndex:end - 1]) &&
      CFStringIsSurrogateLowCharacter([value characterAtIndex:end])) end--;
  return [value substringToIndex:end];
}
// Query-local read budget on the engine's fixed native worker. Do not alter
// write semantics or carry a cancelled query's deadline into a subsequent read.
static _Thread_local DWCancel *queryCancel;
static _Thread_local double queryDeadline;
static _Thread_local BOOL queryTimedOut;
static double monotonicSeconds(void) { return NSProcessInfo.processInfo.systemUptime; }
static BOOL queryStopped(void) {
  if (!queryDeadline) return NO;
  if (cancelled(queryCancel) || monotonicSeconds() >= queryDeadline) {
    queryTimedOut = YES;
    return YES;
  }
  return NO;
}
static BOOL prepareRead(AXUIElementRef e) {
  if (queryStopped()) return NO;
  if (queryDeadline)
    AXUIElementSetMessagingTimeout(e, (float)MIN(0.05, MAX(0.001, queryDeadline - monotonicSeconds())));
  else
    AXUIElementSetMessagingTimeout(e, 0.25);
  return YES;
}
static id attr(AXUIElementRef e, CFStringRef k) {
  if (!prepareRead(e)) return nil;
  CFTypeRef v = NULL;
  if (AXUIElementCopyAttributeValue(e, k, &v) != kAXErrorSuccess)
    return nil;
  return CFBridgingRelease(v);
}
// Fetch the common scalar attributes in one provider call. Some AX providers
// do not implement this API; retain the old per-attribute path for them.
static BOOL wantField(NSArray *fields, NSString *name) { return !fields.count || [fields containsObject:name]; }
static NSDictionary *nodeAttributes(AXUIElementRef e, NSArray *fields) {
  NSMutableArray *names = [NSMutableArray arrayWithObject:(id)kAXRoleAttribute];
  if (wantField(fields, @"name")) [names addObjectsFromArray:@[(id)kAXTitleAttribute, (id)kAXDescriptionAttribute]];
  if (wantField(fields, @"uri")) [names addObjectsFromArray:@[(id)kAXDocumentAttribute, (id)kAXURLAttribute]];
  if (wantField(fields, @"states") || wantField(fields, @"capabilities") || wantField(fields, @"value_preview") || wantField(fields, @"uri")) [names addObject:(id)kAXSubroleAttribute];
  if (wantField(fields, @"states")) [names addObjectsFromArray:@[(id)kAXEnabledAttribute, (id)kAXFocusedAttribute, (id)kAXSelectedAttribute, (id)kAXExpandedAttribute]];
  else if (wantField(fields, @"capabilities")) [names addObject:(id)kAXEnabledAttribute];
  if (wantField(fields, @"bounds")) [names addObjectsFromArray:@[(id)kAXPositionAttribute, (id)kAXSizeAttribute]];
  if (!prepareRead(e)) return nil;
  CFArrayRef raw = NULL;
  AXError rc = AXUIElementCopyMultipleAttributeValues(e, (__bridge CFArrayRef)names, 0, &raw);
  NSMutableDictionary *out = [NSMutableDictionary dictionary];
  if (rc != kAXErrorSuccess || !raw) {
    if (raw) CFRelease(raw);
    for (NSString *name in names) { id v = attr(e, (__bridge CFStringRef)name); if(v) out[name]=v; }
    return out;
  }
  NSArray *values = CFBridgingRelease(raw);
  for (NSUInteger i = 0; i < MIN(names.count, values.count); i++) {
    id v = values[i];
    if (v != NSNull.null && !(CFGetTypeID((__bridge CFTypeRef)v) == AXValueGetTypeID() && AXValueGetType((__bridge AXValueRef)v) == kAXValueAXErrorType)) out[names[i]] = v;
  }
  return out;
}
static id nodeAttr(NSDictionary *batch, AXUIElementRef e, CFStringRef k) {
  return batch[(__bridge NSString *)k];
}
// AppKit's process properties refresh on the main RunLoop. A Go caller may
// never run it. Use live OS queries without taking over the embedding host loop.
static NSArray *processIdentity(pid_t pid) {
  struct proc_bsdinfo info = {0};
  if (pid <= 0 || proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, sizeof(info)) != sizeof(info))
    return nil;
  return @[@(info.pbi_start_tvsec), @(info.pbi_start_tvusec)];
}
// Public Process Manager APIs remain available, though deprecated since 10.9.
// NSWorkspace is not a correct substitute in a headless persistent library.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
static pid_t frontmostPID(void) {
  ProcessSerialNumber psn = {0, kNoProcess};
  pid_t pid = 0;
  return GetFrontProcess(&psn) == noErr && GetProcessPID(&psn, &pid) == noErr ? pid : 0;
}
static NSArray *applicationPIDs(BOOL *complete, DWCancel *cancel) {
  NSMutableArray *pids = [NSMutableArray array];
  ProcessSerialNumber psn = {0, kNoProcess};
  OSErr status;
  while ((status = GetNextProcess(&psn)) == noErr) {
    if (cancelled(cancel) || pids.count >= 1024) { *complete = NO; break; }
    ProcessInfoRec info = {0};
    info.processInfoLength = sizeof(info);
    if (GetProcessInformation(&psn, &info) != noErr) { *complete = NO; continue; }
    if (info.processMode & modeOnlyBackground) continue;
    pid_t pid = 0;
    if (GetProcessPID(&psn, &pid) == noErr && pid > 0)
      [pids addObject:@(pid)];
    else
      *complete = NO;
  }
  if (status != procNotFound && status != noErr) *complete = NO;
  return pids;
}
#pragma clang diagnostic pop
static NSDictionary *captureNode(DWContext *c, NSString *k, NSArray *fields);
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
  if (c.elements.count + c.captureWindows.count >= 10000)
    return @"";
  pid_t pid = 0;
  AXUIElementGetPid(e, &pid);
  NSArray *identity = processIdentity(pid);
  if (!identity) return @"";
  [c.elements addObject:(__bridge id)e];
  [c.meta addObject:@{
    @"App" : app ?: @"",
    @"Window" : win ?: @"",
    @"Parent" : parent ?: @"",
    @"PID" : @(pid),
    @"ProcessIdentity" : identity
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
  DWCaptureWindow *r = c.captureWindows[k];
  if (r) return !r.gone && [r.identity isEqual:processIdentity(r.pid)];
  NSInteger i = k.integerValue - 1;
  if (i < 0 || i >= (NSInteger)c.meta.count)
    return NO;
  NSDictionary *m = c.meta[i];
  NSArray *identity = processIdentity([m[@"PID"] intValue]);
  return identity && [m[@"ProcessIdentity"] isEqual:identity];
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
    @"AXWebArea" : @"document",
    @"AXParagraph" : @"paragraph",
    @"AXHeading" : @"heading",
    @"AXLink" : @"link",
    @"AXImage" : @"image",
    @"AXOutline" : @"list",
    @"AXSheet" : @"container",
    @"AXToolbar" : @"container",
    @"AXSplitGroup" : @"container",
    @"AXGroup" : @"container",
    @"AXScrollArea" : @"container"
  };
  return m[r] ?: @"unknown";
}
// Remote AppKit panels and inline Finder editors may omit AXWindow. Recover
// only from fresh native parent/focus evidence, never a cached title or geometry.
static id frontmostAXApplication(void) {
  pid_t pid = frontmostPID();
  if (!pid) return nil;
  AXUIElementRef app = AXUIElementCreateApplication(pid);
  AXUIElementSetMessagingTimeout(app, 0.25);
  return CFBridgingRelease(app);
}
static id owningWindow(AXUIElementRef e) {
  id direct = attr(e, kAXWindowAttribute);
  if (direct && CFGetTypeID((__bridge CFTypeRef)direct) == AXUIElementGetTypeID())
    return direct;
  id app = frontmostAXApplication();
  id focused = app ? attr((__bridge AXUIElementRef)app, kAXFocusedUIElementAttribute) : nil;
  id window = nil;
  if (focused && CFEqual((__bridge CFTypeRef)focused, e)) {
    if (app)
      window = attr((__bridge AXUIElementRef)app, kAXFocusedWindowAttribute);
  }
  if (window)
    return window;
  id current = (__bridge id)e;
  NSMutableSet *seen = [NSMutableSet set];
  for (int depth = 0; current && depth < 32; depth++) {
    if ([seen containsObject:current])
      break;
    [seen addObject:current];
    AXUIElementRef a = (__bridge AXUIElementRef)current;
    if ([attr(a, kAXRoleAttribute) isEqual:@"AXWindow"])
      return current;
    current = attr(a, kAXParentAttribute);
  }
  return nil;
}
// AXScrollToVisible is a provider-advertised semantic action. Its new AppKit
// constant is macOS 26+, so use the documented wire name without raising our
// deployment target. Providers without this action do not expose the capability.
static CFStringRef scrollToVisibleAction(void) { return CFSTR("AXScrollToVisible"); }
static BOOL elementRect(AXUIElementRef e, CGRect *rect) {
  id pos = attr(e, kAXPositionAttribute), size = attr(e, kAXSizeAttribute);
  CGPoint p; CGSize s;
  if (!pos || !size || CFGetTypeID((__bridge CFTypeRef)pos) != AXValueGetTypeID() ||
      CFGetTypeID((__bridge CFTypeRef)size) != AXValueGetTypeID() ||
      !AXValueGetValue((__bridge AXValueRef)pos, kAXValueCGPointType, &p) ||
      !AXValueGetValue((__bridge AXValueRef)size, kAXValueCGSizeType, &s) ||
      !isfinite(p.x) || !isfinite(p.y) || !isfinite(s.width) || !isfinite(s.height) ||
      s.width <= 0 || s.height <= 0) return NO;
  *rect = CGRectMake(p.x, p.y, s.width, s.height);
  return YES;
}
// Viewport presence is independent of occlusion by other applications. Walk
// only the fresh native parent chain; incomplete ancestry stays unknown.
static NSDictionary *offscreenState(AXUIElementRef e) {
  CGRect visible;
  if (!elementRect(e, &visible)) return unknown();
  id current = attr(e, kAXParentAttribute);
  NSMutableSet *seen = [NSMutableSet set];
  for (int depth = 0; current && depth < 32 && !queryStopped(); depth++) {
    if (CFGetTypeID((__bridge CFTypeRef)current) != AXUIElementGetTypeID()) return unknown();
    if ([seen containsObject:current]) return unknown();
    [seen addObject:current];
    AXUIElementRef parent = (__bridge AXUIElementRef)current;
    NSString *role = attr(parent, kAXRoleAttribute);
    if (![role isKindOfClass:NSString.class]) return unknown();
    if ([role isEqual:@"AXScrollArea"] || [role isEqual:@"AXWindow"]) {
      CGRect clip;
      if (!elementRect(parent, &clip)) return unknown();
      visible = CGRectIntersection(visible, clip);
      if ([role isEqual:@"AXWindow"]) return known(@((BOOL)(CGRectIsNull(visible) || CGRectIsEmpty(visible))));
    }
    current = attr(parent, kAXParentAttribute);
  }
  return unknown();
}
static NSDictionary *checkedState(id value) {
  if ([value isKindOfClass:NSNumber.class] && ([value doubleValue] == 0 || [value doubleValue] == 1))
    return known(@([value boolValue]));
  return unknown(); // Mixed/indeterminate is not false.
}
static NSDictionary *node(DWContext *c, NSString *k, NSArray *fields) {
  if (c.captureWindows[k]) return captureNode(c, k, fields);
  AXUIElementRef e = element(c, k);
  if (!e || !alive(c, k))
    return nil;
  NSDictionary *batch = nodeAttributes(e, fields);
  NSString *role = nodeAttr(batch, e, kAXRoleAttribute);
  if (![role isKindOfClass:NSString.class])
    role = attr(e, kAXRoleAttribute);
  if (![role isKindOfClass:NSString.class])
    return nil;
  NSDictionary *meta = c.meta[k.integerValue - 1];
  NSString *kind = [role isEqual:@"AXApplication"] ? @"application"
                   : [role isEqual:@"AXWindow"]    ? @"window"
                                                   : @"ui";
  NSString *title = nodeAttr(batch, e, kAXTitleAttribute);
  if (![title isKindOfClass:NSString.class] || !title.length)
    title = nodeAttr(batch, e, kAXDescriptionAttribute);
  id nativeURI = nodeAttr(batch, e, kAXDocumentAttribute) ?: nodeAttr(batch, e, kAXURLAttribute);
  if ([nativeURI isKindOfClass:NSURL.class]) nativeURI = [nativeURI absoluteString];
  NSString *sub = nodeAttr(batch, e, kAXSubroleAttribute);
  BOOL protected = [sub isEqual:@"AXSecureTextField"];
  NSMutableDictionary *states = [NSMutableDictionary dictionary];
  NSDictionary *stateAttrs = @{
    @"enabled" : (__bridge NSString *)kAXEnabledAttribute,
    @"focused" : (__bridge NSString *)kAXFocusedAttribute,
    @"selected" : (__bridge NSString *)kAXSelectedAttribute,
    @"expanded" : (__bridge NSString *)kAXExpandedAttribute
  };
  for (NSString *name in stateAttrs) {
    id v = nodeAttr(batch, e, (__bridge CFStringRef)stateAttrs[name]);
    states[name] =
        [v isKindOfClass:NSNumber.class] ? known(@([v boolValue])) : unknown();
  };
  states[@"protected"] = known(@(protected));
  if (queryStopped()) return nil;
  Boolean writable = false;
  AXError writableError = kAXErrorAttributeUnsupported;
 if (wantField(fields, @"states") || wantField(fields, @"capabilities")) writableError = AXUIElementIsAttributeSettable(e, kAXValueAttribute, &writable);
  states[@"read_only"] =
      writableError == kAXErrorSuccess ? known(@((BOOL)!writable)) : unknown();
  // Never ask a protected field for its value, even inside a batch.
  id value = protected || !wantField(fields, @"value_preview") ? nil : attr(e, kAXValueAttribute);
  NSString *valueText = scalarText(value);
  BOOL checkable = [role isEqual:@"AXCheckBox"] && !protected;
  // Chromium's web AX node exposes this provider ID. Its AXValue writability
  // distinguishes actionable checkboxes from aria-readonly checkboxes even
  // though both can advertise AXPress. AppKit press-only checkboxes do not
  // expose this ID and remain actionable.
  BOOL chromiumCheck = checkable && attr(e, CFSTR("ChromeAXNodeId")) != nil;
  id checkedValue = nil;
  if (checkable && (wantField(fields, @"states") || wantField(fields, @"capabilities"))) {
    checkedValue = value ?: attr(e, kAXValueAttribute);
    states[@"checked"] = checkedState(checkedValue);
  }
  id pos = nodeAttr(batch, e, kAXPositionAttribute), sz = nodeAttr(batch, e, kAXSizeAttribute);
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
  if (queryStopped()) return nil;
  CFArrayRef rawActions = NULL;
  if (wantField(fields, @"capabilities") || wantField(fields, @"states")) AXUIElementCopyActionNames(e, &rawActions);
  NSArray *actions = CFBridgingRelease(rawActions);
  BOOL scrollable = [actions containsObject:(__bridge NSString *)scrollToVisibleAction()];
  if (scrollable) states[@"offscreen"] = offscreenState(e);
  Boolean focusable = false;
  if (wantField(fields, @"capabilities")) AXUIElementIsAttributeSettable(e, kAXFocusedAttribute, &focusable);
  BOOL enabled = ![states[@"enabled"][@"Value"] isEqual:@NO];
  NSMutableArray *caps = [NSMutableArray array];
  Boolean expandable = false;
 AXError expandError = kAXErrorAttributeUnsupported;
 if (wantField(fields, @"capabilities")) expandError = AXUIElementIsAttributeSettable(e, kAXExpandedAttribute, &expandable);
  Boolean selectable = false;
  AXError selectError = kAXErrorAttributeUnsupported;
  if (wantField(fields, @"capabilities")) selectError = AXUIElementIsAttributeSettable(e, kAXSelectedAttribute, &selectable);
  NSDictionary *supported = @{
 @"set_expanded": @(expandError == kAXErrorSuccess && expandable),
    @"set_selected": @(selectError == kAXErrorSuccess && selectable),
    // Chromium can report AXValue as settable even when it returns an empty
    // string and does not expose a usable checkbox action. A writable flag
    // alone is not evidence that this desired-state operation is possible.
    @"set_checked": @(checkable && [checkedValue isKindOfClass:NSNumber.class] &&
                      (chromiumCheck
                           ? (writable && [states[@"checked"][@"Status"] isEqual:@"known"] && [actions containsObject:(__bridge NSString *)kAXPressAction])
                           : (writable || ([states[@"checked"][@"Status"] isEqual:@"known"] && [actions containsObject:(__bridge NSString *)kAXPressAction])))),
    @"scroll_into_view": @(scrollable),
    @"focus" : @([kind isEqual:@"window"] || focusable),
    @"invoke" : @([actions containsObject:(__bridge NSString *)kAXPressAction]),
    @"set_value" : @(writable && !protected)
  };
  for (NSString *op in supported) {
    BOOL yes = [supported[op] boolValue];
    if (([op isEqual:@"set_expanded"] || [op isEqual:@"set_checked"] || [op isEqual:@"set_selected"] || [op isEqual:@"scroll_into_view"]) && !yes) continue;
    BOOL stateKnown = ![op isEqual:@"scroll_into_view"] || [states[@"offscreen"][@"Status"] isEqual:@"known"];
    [caps addObject:@{
      @"Name" : op,
      @"Support" : yes ? @"supported" : @"unsupported",
      @"Availability" : yes && enabled && stateKnown ? @"available" : @"blocked"
    }];
  }
  [caps sortUsingComparator:^NSComparisonResult(NSDictionary *a,
                                                NSDictionary *b) {
    return [a[@"Name"] compare:b[@"Name"]];
  }];
  NSString *app = meta[@"App"], *win = meta[@"Window"];
  NSString *parent = meta[@"Parent"];
  if ([kind isEqual:@"ui"]) {
    id actualWindow = owningWindow(e);
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
    @"Fields" : fields ?: @[],
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
          : (valueText ? known(scalarPreview(valueText))
                       : unknown()),
      @"URI" : protected ? @{@"Status" : @"redacted"} : ([nativeURI isKindOfClass:NSString.class] && [nativeURI length] ? known(nativeURI) : unknown()),
      @"States" : states,
      @"Bounds" : bounds,
      @"Capabilities" : caps,
      @"Lifecycle" : @"live"
    }
  };
}
static NSDictionary *seat(DWContext *c) {
  // Some macOS hosts return AXCannotComplete for system-wide focus attributes
  // while the same foreground application's attributes work. Obtain the
  // foreground process with a live OS query and read its AX window/focus directly.
  id app = frontmostAXApplication();
  pid_t sampledPID = 0;
  if (app) AXUIElementGetPid((__bridge AXUIElementRef)app, &sampledPID);
  id focused = app ? attr((__bridge AXUIElementRef)app, kAXFocusedUIElementAttribute) : nil;
  id win =
      app ? attr((__bridge AXUIElementRef)app, kAXFocusedWindowAttribute) : nil;
  NSString *ak =
      app ? key(c, (__bridge AXUIElementRef)app, nil, nil, nil) : @"";
  NSString *wk = win ? key(c, (__bridge AXUIElementRef)win, ak, nil, ak) : @"";
  NSString *fk =
      focused ? key(c, (__bridge AXUIElementRef)focused, ak, wk, wk) : @"";
  if (!sampledPID || sampledPID != frontmostPID()) {
    ak = @"";
    wk = @"";
    fk = @"";
  }
  NSMutableArray *nodes = [NSMutableArray array];
  for (NSString *k in @[ wk, fk ]) {
    // Seat identity does not authorize a hidden value/capability refresh.
    NSDictionary *n = k.length ? node(c, k, @[@"role"]) : nil;
    if (n)
      [nodes addObject:n];
  }
  CGEventRef ev = CGEventCreate(NULL);
  CGPoint p = ev ? CGEventGetLocation(ev) : CGPointZero;
  if (ev)
    CFRelease(ev);
  return @{
    @"Foreground" : wk,
    @"Application" : ak,
    @"Focused" : fk,
    @"Nodes" : nodes,
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
static NSDictionary *pocWindowIdentity(DWContext *c, NSString *targetKey);
#include "capture_darwin.h"
static NSDictionary *queryPage(DWContext *c, NSDictionary *q, DWCancel *cancel) {
  if (!AXIsProcessTrusted())
    return err(@"permission_denied");
  DWScan *scan = nil;
  NSString *resume = q[@"Resume"];
  for (NSString *cursor in [c.scans allKeys])
    if ([((DWScan *)c.scans[cursor]).expires timeIntervalSinceNow] <= 0)
      [c.scans removeObjectForKey:cursor];
  if (resume.length) {
    scan = c.scans[resume];
    if (!scan) return err(@"continuation_expired");
    [c.scans removeObjectForKey:resume];
    // The provider tree is live across calls. A resumed scan can discover
    // targets, but cannot prove that an absent node was never added earlier.
    scan.incomplete = YES;
  } else {
    // A new scan must not evict any retained continuation. The caller can
    // consume an existing cursor or wait for its 90-second expiry.
    if (![q[@"NoContinuation"] boolValue] && c.scans.count >= 16) return scanCapacityError();
    scan = [DWScan new];
    scan.queue = [NSMutableArray array];
    scan.seen = [NSMutableSet set];
    scan.expires = [NSDate dateWithTimeIntervalSinceNow:90];
  }
  NSInteger depth = [q[@"Depth"] integerValue],
            max = [q[@"MaxNodes"] integerValue];
  BOOL summary = [q[@"Summary"] boolValue], detail = [q[@"Detail"] boolValue];
  if (!resume.length && [q[@"Desktop"] boolValue]) {
    BOOL appsComplete = YES;
    NSString *appName = [q[@"AppName"] isKindOfClass:NSString.class] ? q[@"AppName"] : @"";
    for (NSNumber *pid in applicationPIDs(&appsComplete, cancel)) {
      AXUIElementRef e = AXUIElementCreateApplication(pid.intValue);
      AXUIElementSetMessagingTimeout(e, 0.25);
      if (appName.length) {
        // NSRunningApplication.localizedName can differ from the AX name
        // displayed to the Agent (for example, Google Chrome vs Chrome).
        // Read only this candidate's scalar AX name; do not traverse its
        // windows or the unrelated desktop tree.
        id title = attr(e, kAXTitleAttribute);
        if (![title isKindOfClass:NSString.class] || ![title length])
          title = attr(e, kAXDescriptionAttribute);
        if (![title isKindOfClass:NSString.class] || ![title length]) {
          appsComplete = NO;
          CFRelease(e);
          continue;
        }
        if (![title isEqualToString:appName]) {
          CFRelease(e);
          continue;
        }
      }
      NSString *k = key(c, e, nil, nil, nil);
      CFRelease(e);
      if (k.length)
        [scan.queue addObject:@[ k, @0 ]];
      else
        appsComplete = NO;
    }
    if (!appsComplete) scan.incomplete = YES;
  } else if (!resume.length)
    for (NSString *k in q[@"Roots"])
      [scan.queue addObject:@[ k, @0 ]];
  NSMutableArray *nodes = [NSMutableArray array];
  NSInteger chunkVisited = 0;
  while (scan.head < scan.queue.count && chunkVisited < max && scan.visited < 10000) {
    if (queryStopped() || cancelled(cancel)) break;
    NSArray *item = scan.queue[scan.head];
    NSString *k = item[0];
    if ([scan.seen containsObject:k]) {
      scan.head++;
      if (scan.head > 1024) {
        [scan.queue removeObjectsInRange:NSMakeRange(0, scan.head)];
        scan.head = 0;
      }
      continue;
    }
    NSDictionary *n = node(c, k, q[@"Fields"]);
    if (!n) {
      if (queryStopped()) break;
      scan.incomplete = YES;
      [scan.seen addObject:k];
      scan.head++;
      scan.visited++;
      chunkVisited++;
      if (scan.head > 1024) {
        [scan.queue removeObjectsInRange:NSMakeRange(0, scan.head)];
        scan.head = 0;
      }
      continue;
    }
    NSString *kind = n[@"Object"][@"Kind"];
    NSInteger d = [item[1] integerValue];
    if (!detail && d < depth && ![q[@"AppName"] length] && (!summary || [kind isEqual:@"application"])) {
      AXUIElementRef e = element(c, k);
      CFStringRef field = summary ? kAXWindowsAttribute : kAXChildrenAttribute;
      CFIndex count = 0;
      if (!prepareRead(e)) break;
      AXError rc = AXUIElementGetAttributeValueCount(e, field, &count);
      if (rc == kAXErrorSuccess) {
        // Bound both retained keys and pending queue entries, including when
        // a provider repeats the same child under many parents.
        NSInteger room = MAX(0, 10000 - (NSInteger)(scan.queue.count - scan.head));
        CFIndex take = MIN(count, MAX(0, MIN(10000 - (NSInteger)c.elements.count, room)));
        if (take < count) scan.limitHit = YES;
        if (take > 0) {
          CFArrayRef raw = NULL;
          if (!prepareRead(e)) break;
          rc = AXUIElementCopyAttributeValues(e, field, 0, take, &raw);
          if (rc == kAXErrorSuccess) {
            NSMutableArray *children = [NSMutableArray array];
            for (id child in CFBridgingRelease(raw)) {
              if (CFGetTypeID((__bridge CFTypeRef)child) != AXUIElementGetTypeID()) continue;
              NSString *ck = key(c, (__bridge AXUIElementRef)child, n[@"App"], n[@"Window"], k);
              if (ck.length) [children addObject:@[ ck, @(d + 1) ]];
              else scan.limitHit = YES;
              if (queryStopped()) break;
            }
            if (queryStopped()) break;
            [scan.queue addObjectsFromArray:children];
          }
        }
      }
      if (queryStopped()) break;
      if (rc != kAXErrorSuccess && rc != kAXErrorAttributeUnsupported)
        scan.incomplete = YES;
    }
    if (!summary || ![kind isEqual:@"ui"]) [nodes addObject:n];
    [scan.seen addObject:k];
    scan.head++;
    scan.visited++;
    chunkVisited++;
    if (scan.head > 1024) {
      [scan.queue removeObjectsInRange:NSMakeRange(0, scan.head)];
      scan.head = 0;
    }
  }
  if (scan.visited >= 10000 && scan.head < scan.queue.count) scan.limitHit = YES;
  BOOL pending = scan.head < scan.queue.count && scan.visited < 10000;
  NSString *cursor = @"";
  if (pending && ![q[@"NoContinuation"] boolValue] && !cancelled(cancel)) {
    if (scan.head > 1024) {
      [scan.queue removeObjectsInRange:NSMakeRange(0, scan.head)];
      scan.head = 0;
    }
    cursor = NSUUID.UUID.UUIDString;
    c.scans[cursor] = scan;
  }
  BOOL complete = !pending && !scan.incomplete && !scan.limitHit;
  return @{
    @"Result" : @{
      @"Nodes" : nodes,
      @"Complete" : @(complete),
      @"Dirty" : @(scan.incomplete),
      @"Visited" : @(scan.visited),
      @"ScanCursor" : cursor,
      @"Seat" : queryStopped() ? @{@"Pointer": unknown(), @"Health": @"ready", @"Intervention": @"best_effort"} : seat(c),
      @"Unavailable" : complete ? @[] : (scan.limitHit ? @[ @"ax_scan_limit" ] : (queryTimedOut ? @[ @"ax_timeout" ] : (pending ? @[ @"ax_node_budget" ] : @[ @"ax_partial" ])))
    }
  };
}
static NSDictionary *query(DWContext *c, NSDictionary *q, DWCancel *cancel) {
  queryCancel = cancel;
  queryTimedOut = NO;
  long long allowance = [q[@"ReadTimeoutMS"] longLongValue];
  queryDeadline = monotonicSeconds() + (allowance > 0 ? allowance : 2000) / 1000.0;
  @try {
    NSArray *roots = [q[@"Roots"] isKindOfClass:NSArray.class] ? q[@"Roots"] : @[];
    BOOL hasCaptureRoot = NO;
    for (NSString *root in roots) if (c.captureWindows[root]) hasCaptureRoot = YES;
    if (hasCaptureRoot && ![q[@"CaptureWindows"] boolValue]) {
      if (![q[@"Detail"] boolValue]) return err(@"capability_unavailable");
      SCShareableContent *content = shareable(cancel, queryDeadline);
      if (!content) return err(@"provider_unavailable");
      for (NSString *root in roots) if (c.captureWindows[root]) {
        NSString *failure = refreshCaptureWindow(c, root, content);
        if (failure) return err(failure);
      }
    }
    return [q[@"CaptureWindows"] boolValue] ? captureWindowsQuery(c, q, cancel) : queryPage(c, q, cancel);
  } @finally {
    queryCancel = NULL;
    queryDeadline = 0;
  }
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
// The fixed native worker uses this only while a cooperative input call runs.
// A foreground switch or exhausted lease stops new downs/text. Paired releases
// remain necessary to avoid leaving synthetic buttons/modifiers held.
static _Thread_local void *inputPostingContext;
static pid_t cooperativeFrontPID(void);
#ifdef DTW_VIRTUAL_INPUT_POC
// POC route: deliver to the retained target process/window. It deliberately
// never posts to the HID tap or changes the physical cursor position.
static BOOL virtualPostToTarget(DWContext *c, CGEventRef e) {
  if (!c.inputSession) return NO;
  pid_t pid = [c.inputSession[@"targetPID"] intValue];
  CGWindowID wid = [c.inputSession[@"targetWindowID"] unsignedIntValue];
  if (!pid || !wid) return NO;
  CGEventType type = CGEventGetType(e);
  BOOL positioned = type == kCGEventMouseMoved || type == kCGEventLeftMouseDown ||
                    type == kCGEventLeftMouseUp || type == kCGEventRightMouseDown ||
                    type == kCGEventRightMouseUp || type == kCGEventOtherMouseDown ||
                    type == kCGEventOtherMouseUp || type == kCGEventLeftMouseDragged ||
                    type == kCGEventRightMouseDragged || type == kCGEventOtherMouseDragged ||
                    type == kCGEventScrollWheel;
  if (positioned) {
    typedef void (*WindowLocation)(CGEventRef, double, double);
    static WindowLocation setLocation;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
      void *sky = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY);
      setLocation = sky ? (WindowLocation)dlsym(sky, "CGEventSetWindowLocation") : NULL;
    });
    if (!setLocation) return NO;
    NSArray *windows = CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID));
    CGRect rect = CGRectZero;
    BOOL found = NO;
    for (NSDictionary *row in windows) {
      if ([row[(id)kCGWindowNumber] unsignedIntValue] == wid &&
          [row[(id)kCGWindowOwnerPID] intValue] == pid &&
          CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)row[(id)kCGWindowBounds], &rect)) {
        found = YES;
        break;
      }
    }
    CGPoint point = CGEventGetLocation(e);
    if (!found || !CGRectContainsPoint(rect, point)) return NO;
    CGEventSetIntegerValueField(e, (CGEventField)7, 3);
    CGEventSetIntegerValueField(e, (CGEventField)40, pid);
    CGEventSetIntegerValueField(e, (CGEventField)51, wid);
    CGEventSetIntegerValueField(e, (CGEventField)91, wid);
    CGEventSetIntegerValueField(e, (CGEventField)92, wid);
    setLocation(e, point.x - rect.origin.x, point.y - rect.origin.y);
    c.inputSession[@"virtualX"] = @(point.x);
    c.inputSession[@"virtualY"] = @(point.y);
  }
  // Use the same per-PID SkyLight route as background input. Keeping the
  // target in front changes AppKit hit testing, not the physical cursor.
  typedef void (*TargetPost)(pid_t, CGEventRef);
  static TargetPost targetPost;
  static dispatch_once_t postOnce;
  dispatch_once(&postOnce, ^{
    void *sky = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY);
    targetPost = sky ? (TargetPost)dlsym(sky, "SLEventPostToPid") : NULL;
    if (!targetPost) targetPost = CGEventPostToPid;
  });
  targetPost(pid, e);
  if (positioned) {
    CGPoint point = CGEventGetLocation(e);
    fprintf(stderr, "{\"poc\":\"virtual_pointer\",\"x\":%.3f,\"y\":%.3f}\n", point.x, point.y);
  }
  return YES;
}
#endif
static BOOL postInputEvent(CGEventRef e) {
  DWContext *c =
      inputPostingContext ? (__bridge DWContext *)inputPostingContext : nil;
  NSString *heldKey = nil;
  BOOL release = NO;
  if (c && c.inputSession) {
    CGEventType type = CGEventGetType(e);
    BOOL mouseUp = type == kCGEventLeftMouseUp ||
                   type == kCGEventRightMouseUp || type == kCGEventOtherMouseUp;
    BOOL mouseDown = type == kCGEventLeftMouseDown ||
                     type == kCGEventRightMouseDown ||
                     type == kCGEventOtherMouseDown;
    if (mouseUp || mouseDown) {
      heldKey = [NSString
          stringWithFormat:@"b%lld", CGEventGetIntegerValueField(
                                         e, kCGMouseEventButtonNumber)];
      release = mouseUp;
    }
    if (type == kCGEventKeyDown || type == kCGEventKeyUp ||
        type == kCGEventFlagsChanged) {
      CGKeyCode code =
          (CGKeyCode)CGEventGetIntegerValueField(e, kCGKeyboardEventKeycode);
      heldKey = [NSString stringWithFormat:@"k%u", code];
      release = type == kCGEventKeyUp;
      if (type == kCGEventFlagsChanged) {
        CGEventFlags mask = code == 55   ? kCGEventFlagMaskCommand
                            : code == 59 ? kCGEventFlagMaskControl
                            : code == 58 ? kCGEventFlagMaskAlternate
                            : code == 56 ? kCGEventFlagMaskShift
                                         : 0;
        release = mask && !(CGEventGetFlags(e) & mask);
      }
    }
    NSString *failure = c.inputFault;
    if (!failure &&
        cooperativeFrontPID() != [c.inputSession[@"targetPID"] intValue])
      failure = @"user_interrupted";
    if (!failure &&
        monotonicSeconds() >= [c.inputSession[@"deadline"] doubleValue])
      failure = @"input_lease_expired";
    if (failure) {
      c.inputFault = failure;
      if (!release)
        return NO;
      if (heldKey && !c.inputHeld[heldKey])
        return YES;
      if (mouseUp) {
        CGEventRef current = CGEventCreate(NULL);
        if (current) {
          CGEventSetLocation(e, CGEventGetLocation(current));
          CFRelease(current);
        }
      }
    }
  }
#ifdef DTW_VIRTUAL_INPUT_POC
  if (!virtualPostToTarget(c, e)) return NO;
#else
  CGEventPost(kCGHIDEventTap, e);
#endif
  if (heldKey) {
    if (release)
      [c.inputHeld removeObjectForKey:heldKey];
    else
      c.inputHeld[heldKey] = @YES;
  }
  return YES;
}
static BOOL postMouse(CGEventType type, CGPoint p, CGMouseButton b, int count) {
  CGEventRef e = CGEventCreateMouseEvent(NULL, type, p, b);
  if (e) {
    CGEventSetIntegerValueField(e, kCGMouseEventClickState, count);
    BOOL posted = postInputEvent(e);
    CFRelease(e);
    return posted;
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
static NSDictionary *perform(DWContext *, NSDictionary *, DWCancel *);
#include "cooperative_darwin.h"
#ifdef DTW_BACKGROUND_POC
#include "background_poc_darwin.h"
#endif
static NSDictionary *perform(DWContext *c, NSDictionary *o, DWCancel *cancel) {
  if (cancelled(cancel))
    return outcome(@"none", @"cancelled");
  NSDictionary *s = o[@"Step"];
  NSString *op = s[@"Op"], *k = o[@"Key"];
  AXUIElementRef e = element(c, k);
  if (k.length && (!e || !alive(c, k)))
    return outcome(@"none", @"ref_gone");
  if ([op isEqual:@"focus"] || [op isEqual:@"invoke"] ||
      [op isEqual:@"set_value"] || [op isEqual:@"set_expanded"] ||
      [op isEqual:@"set_checked"] || [op isEqual:@"set_selected"] || [op isEqual:@"scroll_into_view"]) {
 if (!AXIsProcessTrusted()) return outcome(@"none", @"permission_denied");
    // Writes such as TextEdit Save/Replace need longer than the 250 ms read
    // budget. A timeout is still unknown and fenced; never replay it. Keep the
    // native action allowance below the default 2 s engine step deadline.
    AXUIElementSetMessagingTimeout(e, 1.0);
    AXError rc = kAXErrorFailure;
    if ([op isEqual:@"invoke"])
      rc = AXUIElementPerformAction(e, kAXPressAction);
    if ([op isEqual:@"set_value"])
      rc = AXUIElementSetAttributeValue(
          e, kAXValueAttribute, (__bridge CFTypeRef)s[@"SetValue"][@"Text"]);
    if ([op isEqual:@"set_expanded"])
 rc = AXUIElementSetAttributeValue(e, kAXExpandedAttribute, [s[@"SetExpanded"][@"Expanded"] boolValue] ? kCFBooleanTrue : kCFBooleanFalse);
    if ([op isEqual:@"set_selected"])
      rc = AXUIElementSetAttributeValue(e, kAXSelectedAttribute, [s[@"SetSelected"][@"Selected"] boolValue] ? kCFBooleanTrue : kCFBooleanFalse);
    if ([op isEqual:@"scroll_into_view"])
      rc = AXUIElementPerformAction(e, scrollToVisibleAction());
    if ([op isEqual:@"set_checked"]) {
      if (![attr(e, kAXRoleAttribute) isEqual:@"AXCheckBox"] || [attr(e, kAXEnabledAttribute) isEqual:@NO]) {
        AXUIElementSetMessagingTimeout(e, 0.25);
        return outcome(@"none", @"capability_unavailable");
      }
      BOOL desired = [s[@"SetChecked"][@"Checked"] boolValue];
      Boolean writable = false;
      AXError support = AXUIElementIsAttributeSettable(e, kAXValueAttribute, &writable);
      CFArrayRef rawActions = NULL;
      AXUIElementCopyActionNames(e, &rawActions);
      NSArray *actions = CFBridgingRelease(rawActions);
      BOOL press = [actions containsObject:(__bridge NSString *)kAXPressAction];
      BOOL chromiumCheck = attr(e, CFSTR("ChromeAXNodeId")) != nil;
      // Read the actual state last, immediately before the possible toggle.
      id value = attr(e, kAXValueAttribute);
      NSDictionary *current = checkedState(value);
      if ([current[@"Status"] isEqual:@"known"] && [current[@"Value"] boolValue] == desired) {
        AXUIElementSetMessagingTimeout(e, 0.25);
        return outcome(@"not_applicable", nil);
      }
      AXUIElementSetMessagingTimeout(e, 1.0);
      if (chromiumCheck && !(support == kAXErrorSuccess && writable && [current[@"Status"] isEqual:@"known"] && press)) {
        AXUIElementSetMessagingTimeout(e, 0.25);
        return outcome(@"none", @"capability_unavailable");
      }
      // An advertised press is the checkbox's semantic change action. Chrome
      // accepts AXValue writes without changing its DOM or dispatching events.
      // Only press from a freshly known opposite state, and never retry it.
      if ([current[@"Status"] isEqual:@"known"] && press)
        rc = AXUIElementPerformAction(e, kAXPressAction);
      else if (support == kAXErrorSuccess && writable && [value isKindOfClass:NSNumber.class])
        rc = AXUIElementSetAttributeValue(e, kAXValueAttribute, desired ? kCFBooleanTrue : kCFBooleanFalse);
      else {
        AXUIElementSetMessagingTimeout(e, 0.25);
        return outcome(@"none", @"capability_unavailable");
      }
    }
    if ([op isEqual:@"focus"]) {
      NSString *role = attr(e, kAXRoleAttribute);
      // Attribute reads restore the ordinary read allowance; focus dispatch
      // retains the existing longer write allowance.
      AXUIElementSetMessagingTimeout(e, 1.0);
      if ([role isEqual:@"AXWindow"]) {
        pid_t pid = 0;
        AXUIElementGetPid(e, &pid);
        [[NSRunningApplication runningApplicationWithProcessIdentifier:pid]
            activateWithOptions:0];
        // AppKit activation is a cooperative request on modern macOS. An
        // explicit accessibility focus operation also requests AXFrontmost;
        // AXRaise alone may succeed while the application stays in background.
        // The engine still verifies the exact foreground window afterwards.
        AXUIElementRef app = AXUIElementCreateApplication(pid);
        AXUIElementSetMessagingTimeout(app, 0.25);
        AXUIElementSetAttributeValue(app, kAXFrontmostAttribute, kCFBooleanTrue);
        CFRelease(app);
        rc = AXUIElementPerformAction(e, kAXRaiseAction);
      } else
        rc = AXUIElementSetAttributeValue(e, kAXFocusedAttribute,
                                          kCFBooleanTrue);
    }
    AXUIElementSetMessagingTimeout(e, 0.25);
    if (rc == kAXErrorSuccess)
      return outcome(@"complete", nil);
    if (rc == kAXErrorCannotComplete || rc == kAXErrorFailure)
      return outcome(@"unknown", @"native_timeout");
    // These advertised state operations have already entered the provider.
    // Even an unsupported/error reply can follow a provider-side effect. Do
    // not invite a second toggle/selection/scroll after uncertain delivery.
    if ([op isEqual:@"set_checked"] || [op isEqual:@"set_selected"] || [op isEqual:@"scroll_into_view"])
      return outcome(@"unknown", @"native_action_failed");
    return outcome(@"none", @"capability_unavailable");
  }
  if (!CGPreflightPostEventAccess()) return outcome(@"none", @"permission_denied");
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
    // The preceding mouse move is asynchronous. Stamp the intended wheel
    // location instead of inheriting the cursor's pre-move position.
    CGEventSetLocation(ev, pt);
    BOOL posted = postInputEvent(ev);
    CFRelease(ev);
    if (!posted)
      return outcome(@"partial", @"input_rejected");
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
      // Chromium ignores Unicode-only newline/tab events. These characters
      // represent the corresponding keyboard key, as in native text editors.
      BOOL special = first == '\n' || first == '\r' || first == '\t';
      CGKeyCode textKey = first == '\t' ? 48 : special ? 36 : 0;
      CGEventRef down = CGEventCreateKeyboardEvent(NULL, textKey, true),
                 up = CGEventCreateKeyboardEvent(NULL, textKey, false);
      if (!down || !up) {
        if (down)
          CFRelease(down);
        if (up)
          CFRelease(up);
        return outcome(i ? @"partial" : @"none", @"input_rejected");
      }
      if (!special) {
        CGEventKeyboardSetUnicodeString(down, count, chars);
        CGEventKeyboardSetUnicodeString(up, count, chars);
      }
      BOOL posted = postInputEvent(down);
      if (posted) postInputEvent(up);
      CFRelease(down);
      CFRelease(up);
      if (!posted)
        return outcome(i ? @"partial" : @"none", @"input_rejected");
      i += count;
      if (first == '\r' && i < text.length && [text characterAtIndex:i] == '\n')
        i++;
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
    NSUInteger postedCount = 0;
    if (valid && !stopped) {
      for (NSUInteger i = 0; i < count; i++) {
        if (postInputEvent(events[i]))
          postedCount++;
        else
          valid = NO;
      }
    }
    for (NSUInteger i = 0; i < count; i++)
      if (events[i])
        CFRelease(events[i]);
    if (!valid || stopped)
      return outcome(postedCount ? @"partial" : @"none", stopped ? @"cancelled" : @"input_rejected");

  }

  else
    return outcome(@"none", @"capability_unavailable");
  return outcome(@"complete", nil);
}
void *dw_open(void) {
  @autoreleasepool {
    DWContext *c = [DWContext new];
    c.elements = [NSMutableArray array];
    c.scans = [NSMutableDictionary dictionary];
    c.captureWindows = [NSMutableDictionary dictionary];
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
// Resolve an AX action key to a live same-PID native window number for
// window-content capture. Never use title or geometry as identity.
static NSDictionary *pocWindowIdentity(DWContext *c, NSString *targetKey) {
  if (!alive(c, targetKey)) return err(@"ref_gone");
  AXUIElementRef target = element(c, targetKey);
  if (!target) return err(@"window_identity_unavailable");
  pid_t pid = 0;
  if (AXUIElementGetPid(target, &pid) != kAXErrorSuccess || pid <= 0)
    return err(@"window_identity_unavailable");
  id current = (__bridge id)target;
  id window = nil;
  NSMutableSet *seen = [NSMutableSet set];
  for (int depth = 0; current && depth < 32; depth++) {
    if ([seen containsObject:current]) break;
    [seen addObject:current];
    AXUIElementRef candidate = (__bridge AXUIElementRef)current;
    pid_t candidatePID = 0;
    if (AXUIElementGetPid(candidate, &candidatePID) != kAXErrorSuccess || candidatePID != pid)
      break;
    if ([attr(candidate, kAXRoleAttribute) isEqual:(__bridge NSString *)kAXWindowRole]) {
      window = current;
      break;
    }
    id direct = attr(candidate, kAXWindowAttribute);
    if (direct && CFGetTypeID((__bridge CFTypeRef)direct) == AXUIElementGetTypeID()) {
      pid_t windowPID = 0;
      if (AXUIElementGetPid((__bridge AXUIElementRef)direct, &windowPID) == kAXErrorSuccess && windowPID == pid &&
          [attr((__bridge AXUIElementRef)direct, kAXRoleAttribute) isEqual:(__bridge NSString *)kAXWindowRole]) {
        window = direct;
        break;
      }
    }
    current = attr(candidate, kAXParentAttribute);
  }
  if (!window) return err(@"window_identity_unavailable");
  typedef AXError (*WindowID)(AXUIElementRef, CGWindowID *);
  WindowID getWindow = (WindowID)dlsym(RTLD_DEFAULT, "_AXUIElementGetWindow");
  CGWindowID wid = 0;
  if (!getWindow || getWindow((__bridge AXUIElementRef)window, &wid) != kAXErrorSuccess || !wid)
    return err(@"window_identity_unavailable");
  NSArray *identity = processIdentity(pid);
  if (identity.count != 2) return err(@"window_identity_unavailable");
  NSArray *list = CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionAll, kCGNullWindowID));
  NSInteger matches = 0;
  for (NSDictionary *row in list)
    if ([row[(id)kCGWindowNumber] unsignedIntValue] == wid &&
        [row[(id)kCGWindowOwnerPID] intValue] == pid) matches++;
  if (matches != 1) return err(@"window_identity_unavailable");
  id app = CFBridgingRelease(AXUIElementCreateApplication(pid));
  NSString *appKey = key(c, (__bridge AXUIElementRef)app, nil, nil, nil);
  NSString *windowKey = key(c, (__bridge AXUIElementRef)window, appKey, nil, appKey);
  if (!appKey.length || !windowKey.length) return err(@"window_identity_unavailable");
  return @{ @"Result": @{ @"PID": @(pid), @"StartSec": identity[0], @"StartUSec": identity[1],
    @"NativeWindowID": @(wid), @"AppKey": appKey, @"WindowKey": windowKey } };
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
                @"Support" : @"supported",
                @"Availability" : CGPreflightScreenCaptureAccess() ? @"available" : @"blocked"
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
        NSString *captureFailure = nil;
        if (c.captureWindows[r[@"Key"]]) {
          if (!CGPreflightScreenCaptureAccess()) captureFailure = @"permission_denied";
          else {
            SCShareableContent *content = shareable((DWCancel *)cancel, monotonicSeconds()+2);
            captureFailure = content ? refreshCaptureWindow(c, r[@"Key"], content) : @"provider_unavailable";
          }
        }
        NSDictionary *n = captureFailure ? nil : node(c, r[@"Key"], nil);
        out = n
                  ? @{@"Result" : @{@"Node" : n, @"Seat" : seat(c)}}
                  : captureFailure ? err(captureFailure) : err(alive(c, r[@"Key"]) ? @"provider_unavailable"
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
          id v = scalarText(attr(e, kAXValueAttribute));
          if (!v) {
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
      }
#ifdef DTW_POC_EXACTGRANT
      else if ([op isEqual:@"poc_window_identity"])
        out = pocWindowIdentity(c, r[@"Key"]);
#endif
      else if ([op isEqual:@"perform"] || [op isEqual:@"cooperative_perform"] ||
               [op isEqual:@"background_poc"]) {
        NSString *failure = terminalTargetFailure(c, r);
        if ([failure isEqual:@"terminal_application_blocked"])
          out = @{ @"Fault": @{ @"Code": failure,
            @"Message": @"DTW does not operate terminal applications",
            @"RetryClass": @"never_automatically" } };
        else if (failure) out = err(failure);
        else if ([op isEqual:@"perform"]) out = perform(c, r, (DWCancel *)cancel);
        else if ([op isEqual:@"cooperative_perform"]) out = cooperativePerform(c, r, (DWCancel *)cancel);
#ifdef DTW_BACKGROUND_POC
        else out = backgroundPOC(c, r, (DWCancel *)cancel);
#else
        else out = err(@"background_unavailable");
#endif
      }
      else if ([op isEqual:@"input_end"]) {
        out = cooperativeEnd(c);
        c.inputFault = nil;
      }
      else if ([op isEqual:@"input_begin"]) {
        if (c.inputSession)
          out = err(@"seat_fenced");
        else {
          c.inputReport = nil;
          c.inputFault = nil;
          out = @{@"Result" : @YES};
        }
      }
      else if ([op isEqual:@"input_available"])
        out = @{@"Result" : @(cooperativeAvailable())};
      else if ([op isEqual:@"input_guard"]) {
        NSString *failure = cooperativeGuard(c);
        out = failure ? err(failure) : @{@"Result" : @YES};
      }
      else if ([op isEqual:@"hit"]) {
        AXUIElementRef sys = AXUIElementCreateSystemWide(), hit = NULL;
        NSDictionary *p = r[@"Point"];
        BOOL ok = AXUIElementCopyElementAtPosition(sys, [p[@"X"] floatValue],
                                                   [p[@"Y"] floatValue],
                                                   &hit) == kAXErrorSuccess;
        AXUIElementRef wanted = element(c, r[@"Key"]);
        // A container/window anchor is hittable through a native descendant;
        // an unrelated occluding window still fails this ancestry check.
        BOOL descendant = NO;
        id current = hit ? (__bridge id)hit : nil;
        NSMutableSet *seen = [NSMutableSet set];
        for (int depth = 0; ok && wanted && current && depth < 64; depth++) {
          if (CFEqual((__bridge CFTypeRef)current, wanted)) {
            descendant = YES;
            break;
          }
          if ([seen containsObject:current])
            break;
          [seen addObject:current];
          current = attr((__bridge AXUIElementRef)current, kAXParentAttribute);
        }
        ok = ok && descendant;
        if (hit)
          CFRelease(hit);
        CFRelease(sys);
        out = @{@"Result" : @(ok)};
      } else if ([op isEqual:@"capture"])
        out = capture(c, r, (DWCancel *)cancel);
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
      if (getenv("DTW_POC_NATIVE_TRACE"))
        fprintf(stderr, "DTW native exception: %s\n", exception.name.UTF8String ?: "unknown");
      return strdup("{\"Fault\":{\"Code\":\"provider_unavailable\",\"Message\":"
                    "\"native exception\",\"RetryClass\":\"reobserve\"}}");
    }
  }
}
