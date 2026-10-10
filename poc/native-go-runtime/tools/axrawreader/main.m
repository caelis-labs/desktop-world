// Independent Objective-C/CoreFoundation AX wire probe for one owned fixture PID.
// No Go bridge, Swift collection cast, registry key, JSON serializer, or App title.
#import <ApplicationServices/ApplicationServices.h>
#import <CoreFoundation/CoreFoundation.h>
#import <dlfcn.h>
#import <stdio.h>
#import <stdlib.h>

typedef AXError (*NativeWindowID)(AXUIElementRef, CGWindowID *);

static void printRole(AXUIElementRef element, const char *label) {
  CFTypeRef role = NULL;
  AXError status = AXUIElementCopyAttributeValue(element, kAXRoleAttribute, &role);
  char roleBytes[96] = "unavailable";
  if (status == kAXErrorSuccess && role && CFGetTypeID(role) == CFStringGetTypeID())
    CFStringGetCString((CFStringRef)role, roleBytes, sizeof(roleBytes), kCFStringEncodingUTF8);
  pid_t owner = 0;
  AXError pidStatus = AXUIElementGetPid(element, &owner);
  CGWindowID native = 0;
  NativeWindowID getWindow = (NativeWindowID)dlsym(RTLD_DEFAULT, "_AXUIElementGetWindow");
  AXError nativeStatus = getWindow ? getWindow(element, &native) : kAXErrorNotImplemented;
  printf("%s role=%s role_status=%d pid=%d pid_status=%d native=%u native_status=%d\n",
         label, roleBytes, status, owner, pidStatus, native, nativeStatus);
  if (role) CFRelease(role);
}

int main(int argc, char **argv) {
  if (argc != 2) { fputs("usage: AXRawReader OWNED_FIXTURE_PID\n", stderr); return 2; }
  char *end = NULL;
  long parsed = strtol(argv[1], &end, 10);
  if (!end || *end || parsed <= 0 || parsed > INT32_MAX) return 2;
  pid_t pid = (pid_t)parsed;
  AXUIElementRef app = AXUIElementCreateApplication(pid);
  printf("reader_trusted=%d reader_pid=%d target_pid=%d app_type=%lu expected_ax_type=%lu\n",
         AXIsProcessTrusted(), getpid(), pid, CFGetTypeID(app), AXUIElementGetTypeID());
  printRole(app, "app_before");
  CFTypeRef result = NULL;
  AXError status = AXUIElementCopyAttributeValue(app, kAXWindowsAttribute, &result);
  CFTypeID resultType = result ? CFGetTypeID(result) : 0;
  CFIndex count = result && resultType == CFArrayGetTypeID() ? CFArrayGetCount((CFArrayRef)result) : 0;
  printf("windows_status=%d out_pointer_nonnull=%d result_type=%lu expected_array_type=%lu count=%ld\n",
         status, result != NULL, resultType, CFArrayGetTypeID(), count);
  if (count > 8) count = 8;
  for (CFIndex i = 0; i < count; i++) {
    CFTypeRef borrowed = CFArrayGetValueAtIndex((CFArrayRef)result, i);
    CFTypeID type = borrowed ? CFGetTypeID(borrowed) : 0;
    printf("child_index=%ld child_type=%lu child_ptr_equals_app=%d child_cf_equals_app=%d\n",
           i, type, borrowed == app, borrowed && CFEqual(borrowed, app));
    if (type != AXUIElementGetTypeID()) continue;
    // Keep the child alive after releasing the provider-owned array. The
    // subsequent role/PID/native-ID read cannot accidentally use a dead child.
    AXUIElementRef child = (AXUIElementRef)CFRetain(borrowed);
    printRole(child, "child_before_array_release");
    CFRelease(result);
    result = NULL;
    printRole(child, "child_after_array_release");
    printRole(app, "app_after_array_release");
    CFRelease(child);
    break;
  }
  if (result) CFRelease(result);
  CFRelease(app);
  return status == kAXErrorSuccess ? 0 : 1;
}
