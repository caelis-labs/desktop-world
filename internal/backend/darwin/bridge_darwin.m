// Native references belong to the engine's fixed worker thread. No
// NSApplication is created and no main-thread event loop is taken over by the
// library.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <Carbon/Carbon.h>
#import <ImageIO/ImageIO.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#include <libproc.h>
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
@property NSMutableDictionary *scans;
@property NSArray *lastDisplays;
@property NSUInteger topology;
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
static NSDictionary *node(DWContext *c, NSString *k, NSArray *fields) {
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
  if (wantField(fields, @"capabilities")) AXUIElementCopyActionNames(e, &rawActions);
  NSArray *actions = CFBridgingRelease(rawActions);
  Boolean focusable = false;
  if (wantField(fields, @"capabilities")) AXUIElementIsAttributeSettable(e, kAXFocusedAttribute, &focusable);
  BOOL enabled = ![states[@"enabled"][@"Value"] isEqual:@NO];
  NSMutableArray *caps = [NSMutableArray array];
  Boolean expandable = false;
 AXError expandError = kAXErrorAttributeUnsupported;
 if (wantField(fields, @"capabilities")) expandError = AXUIElementIsAttributeSettable(e, kAXExpandedAttribute, &expandable);
  NSDictionary *supported = @{
 @"set_expanded": @(expandError == kAXErrorSuccess && expandable),
    @"focus" : @([kind isEqual:@"window"] || focusable),
    @"invoke" : @([actions containsObject:(__bridge NSString *)kAXPressAction]),
    @"set_value" : @(writable && !protected)
  };
  for (NSString *op in supported) {
    BOOL yes = [supported[op] boolValue];
    if ([op isEqual:@"set_expanded"] && !yes) continue;
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
    if (c.scans.count >= 16) return scanCapacityError();
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
    for (NSNumber *pid in applicationPIDs(&appsComplete, cancel)) {
      AXUIElementRef e = AXUIElementCreateApplication(pid.intValue);
      AXUIElementSetMessagingTimeout(e, 0.25);
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
    if (!detail && d < depth && (!summary || [kind isEqual:@"application"])) {
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
  if (pending) {
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
    return queryPage(c, q, cancel);
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
  if ([op isEqual:@"focus"] || [op isEqual:@"invoke"] ||
      [op isEqual:@"set_value"] || [op isEqual:@"set_expanded"]) {
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
    c.scans = [NSMutableDictionary dictionary];
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
        NSDictionary *n = node(c, r[@"Key"], nil);
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
      } else if ([op isEqual:@"perform"])
        out = perform(c, r, (DWCancel *)cancel);
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
