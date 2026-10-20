# Pre-release terminal checklist

Before each release, check by hand that Whetstone keeps its [terminal support](../README.md#terminal-support) claim in every supported terminal. CI already checks the same behaviour on every change. This checklist covers what CI can't: the real terminals, their fonts and colours, tmux and SSH.

## Runs

Work through [the steps](#steps) once for each supported terminal, run three ways.

- **Directly:** from a shell in the terminal itself.
- **In tmux:** from a shell in a tmux pane, inside the terminal, with `escape-time` set low, as the [README's tip](../README.md#terminal-support) says.
- **Over SSH:** from a shell on another Linux or macOS machine, reached with `ssh` from the terminal.

Always start `whetstone` from a shell, never as a tmux pane's own command or with `ssh -t host whetstone`. Step 5 needs a shell to suspend to.

Mark each run once all six steps pass:

| Terminal | Platform | Directly | In tmux | Over SSH |
| --- | --- | --- | --- | --- |
| iTerm2 | macOS | | | |
| Ghostty | macOS | | | |
| Terminal.app | macOS | | | |
| GNOME Terminal | Linux | | | |
| Ghostty | Linux | | | |

## Before you start

- Install the build you are about to release, on this machine and on the SSH host.
- Start Whetstone in a directory outside any Git repository, so home offers the showcase project first.
- Copy this text, three lines with keys that would act if they arrived as keys, for step 3:

  ```text
  retry
  q j :
  identity
  ```

## Steps

Each step names the CI tests that check the same behaviour, in a pseudo-terminal or at the app seam, under `crates/whetstone/tests/it/`. A step that fails here while its tests pass points at the terminal, tmux or SSH.

### 1. Start, and one Practice at 80×24

Set the window to 80 columns by 24 rows (`stty size` prints `24 80`), and run `whetstone`.

1. Home shows whole, with the logo, **Practice on the showcase project**, **Settings** and **Quit**, and the key hints at the bottom.
2. Choose **Practice on the showcase project**. The areas and the showcase project's changes show whole, and **Generate questions** is disabled until Jev arrives.
3. Move to **Preserve payment retry identity** and press Enter. The change opens, with its files on the left and its diff on the right.
4. `l` and `h` move between the panes, and `j`/`k` move through each.

It passes when every screen fits, borders are unbroken lines, nothing overlaps, and the focused control stands out in colour.

CI: `every_screen_and_overlay_fits_whole_at_120x32_100x28_and_80x24` (`resize.rs`).

### 2. Shrink below 50×12, and back

With the change open and its diff scrolled down a little, shrink the window below 50 columns or 12 rows.

1. "Window too small" shows, with the window's size.
2. `q` does nothing.
3. Grow the window back to 80×24. The change comes back as you left it: the same pane focused and the same scroll.

CI: `below_50x12_the_notice_shows_and_growing_back_brings_the_screen_back` (`pty_terminal.rs`), and `keys_do_nothing_while_the_window_is_too_small` and `resizing_back_restores_the_screen_and_focus` (`resize.rs`).

### 3. A multi-line paste

On the open change, press `/` to search the diff, and paste the text you copied.

1. The search box shows `retry q j : identity` on one line. Nothing was searched, and the change is still open.
2. Press Esc to close the search, and paste again. Nothing happens.

CI: `a_real_paste_with_a_line_break_neither_searches_nor_goes_back_and_is_never_logged` (`pty_paste.rs`).

### 4. Running with `NO_COLOR=1`

Quit with `q` until the shell is back, and run `NO_COLOR=1 whetstone`.

1. Nothing is coloured: text is in the terminal's own colours.
2. Focus still shows on home, through the `>` marker and bold text.
3. Open the change as in step 1. The chosen file and the focused pane still show, without colour.

Quit with `q` until the shell is back.

CI: `with_no_color_nothing_is_coloured_and_focus_stays_bold` (`pty.rs`).

### 5. Ctrl+Z, then `fg`

Run `whetstone`, open the change, scroll its diff, and type `/retry` without pressing Enter. Press Ctrl+Z.

1. The shell's prompt is back, as it was before Whetstone started, and the shell says the job stopped. The cursor shows, and what you type shows as you type it.
2. Resize the window.
3. Run `fg`. The same screen comes back, drawn in full at the window's new size: the change, the same pane and scroll, and `retry` in the search box.

CI: `ctrl_z_gives_the_terminal_back_and_fg_brings_back_the_same_screen` and `a_resize_while_suspended_is_honoured_after_fg` (`pty_suspend.rs`).

### 6. Ctrl+C

With the search box still open, press Ctrl+C.

1. Whetstone quits at once, without a question.
2. The shell's prompt is back, as it was before Whetstone started. The cursor shows, and what you type shows as you type it.
3. `echo $?` prints `0`.

CI: `ctrl_c_quits_and_restores_the_terminal` (`pty.rs`).

## When a step fails

Report it as a bug. Name the terminal and its version, the platform, how Whetstone ran (directly, in tmux with its version, or over SSH), the step, and what you saw.
