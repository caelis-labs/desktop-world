# Desktop Computer Use Safety & Confirmation Policy

Because desktop UI automation directly triggers external side effects through live user interface actions, agents must adhere to this safety and confirmation policy.

## Confirmation Modes

### 1. Hand-Off Required (User Must Do It)
The agent must stop and explicitly ask the user to take over for:
- Entering or changing OS account passwords, master vault passwords, or disk encryption credentials.
- Operating biometric authentication prompts (e.g., macOS Touch ID dialogs) or system elevation prompts (e.g., Windows UAC elevation).
- Entering primary financial payment credentials (e.g., raw credit card numbers or CVV).
- Bypassing critical security warnings (e.g., expired/invalid SSL certificates, untrusted executable gatekeeper bypasses).

### 2. Always Confirm at Action-Time (Even If Pre-Approved)
A blocking confirmation is required immediately before executing the UI action:
- **Irreversible Deletion**:
  - Emptying system Trash / Recycle Bin via GUI.
  - Deleting local directories, database files, or repositories through desktop file managers.
  - Deleting cloud-synced emails, social posts, or documents in GUI clients.
- **Outbound Communication & Publishing**:
  - Sending emails or submitting public comments/messages in desktop messengers (e.g., Slack, WeChat, Teams).
  - Submitting high-stakes forms (job applications, financial transfers, official filings).
- **System & Security Settings**:
  - Modifying system proxy, VPN, firewall, or security permission settings.
- **Installing New Software**:
  - Clicking through installers for newly downloaded software or installing untrusted browser extensions.

### 3. Pre-Approval Works (Otherwise Treat as "Always Confirm")
If the user's initial prompt explicitly requested the specific action, proceed without re-confirming; otherwise confirm immediately before action:
- Logging into known web services where login is directly implied by the task.
- Uploading local files to a web form or cloud client when requested.
- Closing an application or document with uncommitted changes (when requested).
- Overwriting existing files during Save / Export dialog flows.

### 4. No Confirmation Needed (Always Allowed)
- Read-only actions: inspecting AX trees, listing windows, querying controls, taking screenshots.
- In-document navigation, scrolling, selection, and reading.
- Typing and editing inside temporary/scratch documents explicitly designated by the user.
- Dismissing standard cookie banners or non-destructive informational dialogs.

## Confirmation Hygiene
- **Explain Impact**: Clearly state what UI action is about to happen, which application will receive it, and what the consequences are.
- **Don't Ask Prematurely**: Complete all read-only inspection and preparation first; only prompt for confirmation at the exact moment before dispatching the high-impact action.
- **No Blanket Approvals**: A vague instruction (e.g., "clean up my desktop") is not pre-approval for deleting files.
