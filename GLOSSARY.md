# Gopu

A local-first desktop companion. It drafts, polishes or carries out text in whatever field the user is typing in, and it remembers what was on screen.

## The Gopu key

**Gopu key**:
The single system-wide hotkey that asks Gopu to act on the focused field.
_Avoid_: hotkey, shortcut, trigger, /g

**Field**:
The editable text control in another app that has keyboard focus when the Gopu key is tapped.
_Avoid_: text box, input, textarea, edit control

**Caret rectangle**:
The on-screen rectangle of the insertion point inside the field, as reported by the OS.
_Avoid_: cursor position, caret pos

**Field rectangle**:
The on-screen bounds of the field as a whole. Gopu walks along its top border.
_Avoid_: field bounds, element rect

## Field cases

**Field case**:
Which of four mutually exclusive situations the field is in when the Gopu key is tapped: selection, empty, draft or instruction. Exactly one applies per tap, checked in that order.
_Avoid_: mode, intent, action type

**Selection**:
The field case where the user has highlighted text. Gopu rewrites only the highlighted text.
_Avoid_: highlight, slice

**Empty**:
The field case where the field holds no user text. Gopu writes the message the on-screen context implies.
_Avoid_: blank, new message

**Draft**:
The field case where the user's text is their own prose. Gopu polishes it.
_Avoid_: rough text, user text

**Instruction**:
The field case where the user's text is a request addressed to Gopu. Gopu carries it out and the result replaces the request. The model, not a local rule, tells an instruction apart from a draft.
_Avoid_: command, prompt

## Context and write-back

**On-screen context**:
Text readable from the focused window at the moment of the tap, anchored around the field. It is untrusted data, never instructions.
_Avoid_: screen text, surrounding text, page content

**Recall**:
Anything pulled from memory to inform a result. Kept apart from on-screen context because its trust and privacy differ.
_Avoid_: context (unqualified), history

**Target**:
The part of the field that a write-back replaces: the selection if there is one, otherwise the user's text.
_Avoid_: slice, range, region

**Kept text**:
Text in the field that is not the user's draft, such as a quoted thread or signature. A write-back never touches it.
_Avoid_: tail, footer, protected text

**Write-back**:
Putting Gopu's result into the field in place of the target.
_Avoid_: paste-back, insert, apply

## Memory

**Memory**:
The local, searchable store of on-screen text that Gopu captures in the background (v1.1).
_Avoid_: recall (that is the act of reading from it), history, timeline

**Capture**:
Gopu reading the foreground window's text into memory in the background, without a Gopu-key tap. Also one stored reading of a window.
_Avoid_: snapshot, recording, screenshot

**Never-capture list**:
The built-in rules for windows Gopu never captures, such as sign-in pages, password managers, private windows and banks. The user can't remove entries.
_Avoid_: blocklist, denylist

**Ignore list**:
The user's own apps and sites that Gopu stops capturing. Adding one also forgets what was already captured from it. It affects memory only, never the Gopu key.
_Avoid_: exclusions, per-app off switch

**Mask**:
Replacing a secret (a card number, key, ID number) with a placeholder before it is stored, so the secret never reaches memory.
_Avoid_: redact, scrub, censor

**Key result**:
What Gopu remembers about one Gopu-key tap: where it happened, the user's text and Gopu's result. It is kept only where memory is allowed.
_Avoid_: output log, tap log (that is the debug file), rewrite

**Sent pair**:
A key result matched with the text the user actually sent from that field. Its verdict is kept, edited, replaced or unseen. Sent pairs are how Gopu learns the user's voice.
_Avoid_: draft→sent pair, send pair, edit pair

**People page**:
A short page Gopu keeps about someone the user writes to often: names, who they are, the language used with them, and dated facts. It is used only to get replies right about that person.
_Avoid_: wiki page, contact, profile, entity

**Forget**:
Wiping part of memory for good, such as the last 15 minutes, one result or everything, so that it can't be found or recovered.
_Avoid_: delete history, clear, hide

**Style rule**:
A short, lasting rule about how the user writes, learned from sent pairs or from an instruction like "never use emojis". It applies to one app or site, or everywhere. The user's own style notes win over it.
_Avoid_: preference, habit, template

**Kept rate**:
How often the user sent Gopu's result unchanged, per app. It shows whether Gopu is learning the user's voice.
_Avoid_: accuracy, acceptance rate, score

**Access log**:
Which other AI app read memory through the MCP server, and when (the tool, never the query or the result). Kept in its own file; each app can be cut off from it.
_Avoid_: audit trail, history

**Pause**:
Stopping capture until a set time. The Gopu key keeps working.
_Avoid_: disable, sleep, mute

## The overlay

**Overlay**:
Gopu's transparent, click-through window drawn above other apps for the duration of one Gopu-key run.
_Avoid_: popup, HUD, floating window

**Gopu Walk**:
The animation shown in the overlay: Gopu drops onto the field rectangle's top border, walks while writing, and hops when done.
_Avoid_: loader, spinner, animation

**Bubble**:
Gopu's speech bubble. It is the only status display ("Thinking…", "Polishing…", "Done!", "Oops. Your draft is safe.").
_Avoid_: toast, status panel, tooltip
