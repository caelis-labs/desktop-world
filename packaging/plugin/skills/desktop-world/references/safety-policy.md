# Desktop Computer Use Safety & Confirmation Policy

Desktop World operates as an embedded automation plugin. This policy provides safety guardrails for live UI actions while avoiding unnecessary friction.

## Host Policy Precedence

**Host Policy > Skill Guidance**: Host-level security, sandboxing, and confirmation policies take strict precedence. If the host environment or platform already manages authorization or prompts the user, do NOT generate redundant secondary confirmation hurdles.

---

## Authorization & Confirmation Modes

### 1. Hand-Off Required (User Must Complete)
The agent must stop and ask the user to take over for actions that should never be performed programmatically:
- Entering or modifying OS master passwords, disk encryption keys, or biometric prompts (e.g., macOS Touch ID / Windows Hello).
- Operating system-level elevation dialogs (e.g., Windows UAC prompts, macOS sudo/authorization alerts).
- Entering primary financial credentials (e.g., raw credit card numbers or CVV).
- Bypassing critical security warnings (e.g., invalid SSL certificates, unrecognized executable security barriers).

### 2. Always Confirm at Action-Time (Even If Broadly Pre-Approved)
A blocking confirmation is required immediately before executing the UI action when the side effect is permanent and high-risk:
- **Permanent & Irreversible Destruction**:
  - Permanently deleting files (e.g., Emptying Trash / Recycle Bin via GUI, bypass-trash permanent deletion).
  - Deleting entire projects, remote Git repositories, or production databases through desktop clients.
- **Unauthorized External Communication**:
  - Submitting public messages, comments, or external emails when the user prompt did NOT explicitly specify sending the message.
  - Submitting formal applications, tax filings, or financial transactions.
- **System-Wide Configuration**:
  - Modifying system proxy, VPN, firewall, or OS security settings via desktop settings.

### 3. Pre-Approved via Explicit Prompt Intent (Proceed Smoothly)
If the user's initial prompt explicitly requests the action, execute it directly without redundant re-confirmation:
- **Requested Communications**: Clicking "Send" in a messenger or email client when the user specifically instructed: *"Send this message to Bob"*.
- **Document & Content Editing**: Editing, formatting, saving, and overwriting files in documents or applications explicitly designated by the user.
- **Reversible File Operations**: Moving files to Trash / Recycle Bin as part of an explicit organization request.
- **Application Navigation & Form Filling**: Entering non-sensitive inputs, logging into services where login is directly implied by the task, and clicking standard confirmation buttons (e.g., "OK", "Apply").

### 4. No Confirmation Needed (Always Permitted)
- **Scoped Read-Only Observation**: Inspecting AX trees, window lists, and capturing screenshots **strictly within the authorized application PID**.
- In-document navigation, scrolling, selection, and reading.
- Dismissing standard cookie banners, non-destructive informational popups, or tooltips.

---

## Observation Scope Hygiene

- **Stay Within Scope**: Keep observations and screenshots focused on the authorized target application and PID. Avoid unprompted full-desktop observations (`operation: "desktop"`) unless specifically needed for system-level dialogs and authorized by the task.
- **Just-in-Time Confirmation**: If a dangerous action requires confirmation, perform all non-destructive preparation first, then explain the exact impact and target application immediately before dispatching the final action.
