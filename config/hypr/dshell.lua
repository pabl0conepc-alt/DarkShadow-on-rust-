-- DarkShadow's Shell (Rust) for Hyprland 0.55+ (Lua config).
-- Put this file in ~/.config/hypr/ and add `require("dshell")` to hyprland.lua.
-- pcall: a key this Hyprland does not know is skipped instead of breaking the config.

hl.on("hyprland.start", function()
    hl.exec_cmd("dshell")
end)

-- blur behind the shell's glass (the wallpaper and the visualizer stay sharp)
for _, ns in ipairs({ "dshell-bar", "dshell-island", "dshell-launcher", "dshell-control", "dshell-power", "dshell-picker", "dshell-widget" }) do
    pcall(hl.layer_rule, { name = ns .. "-blur", match = { namespace = ns }, blur = true, ignore_alpha = 0.25 })
end
-- the shell animates its own panels; Hyprland's layer animation would play on top of it
for _, ns in ipairs({ "dshell-wallpaper", "dshell-island", "dshell-launcher", "dshell-control", "dshell-power", "dshell-picker", "dshell-viz" }) do
    pcall(hl.layer_rule, { name = ns .. "-noanim", match = { namespace = ns }, no_anim = true })
end

local function bind(keys, cmd, opts)
    pcall(hl.unbind, keys)
    pcall(hl.bind, keys, hl.dsp.exec_cmd(cmd), opts)
end

-- the keys of DarkShadow's Shell
bind("ALT + Space", "dshell toggle launcher")
bind("SUPER + R", "dshell toggle launcher")
bind("SUPER + N", "dshell toggle control")
bind("SUPER + A", "dshell toggle control")
bind("SUPER + W", "dshell toggle picker")
bind("SUPER + V", "dshell toggle clipboard")
bind("SUPER + ALT + W", "dshell edit")
bind("SUPER + ALT + D", "dshell toggle today")
bind("SUPER + U", "dshell toggle settings")
bind("SUPER + G", "dshell gaming")
bind("CTRL + ALT + Delete", "dshell toggle power")
bind("SUPER + L", "dshell lock")

-- screenshots and recording (press the recording key again to stop)
bind("Print", "dshell shot region")
bind("SHIFT + Print", "dshell shot full")
bind("SUPER + SHIFT + S", "dshell shot region")
bind("SUPER + SHIFT + R", "dshell record region")
bind("SUPER + ALT + R", "dshell record full-sound")

-- volume, microphone, brightness and media (shown in the island)
bind("XF86AudioRaiseVolume", "dshell volume up", { locked = true })
bind("XF86AudioLowerVolume", "dshell volume down", { locked = true })
bind("XF86AudioMute", "dshell volume mute", { locked = true })
bind("XF86AudioMicMute", "dshell mic mute", { locked = true })
bind("SUPER + F12", "dshell volume up")
bind("SUPER + F11", "dshell volume down")
bind("SUPER + F10", "dshell volume mute")
bind("XF86MonBrightnessUp", "dshell brightness up", { locked = true })
bind("XF86MonBrightnessDown", "dshell brightness down", { locked = true })
bind("XF86AudioPlay", "dshell media play-pause", { locked = true })
bind("XF86AudioPause", "dshell media play-pause", { locked = true })
bind("XF86AudioNext", "dshell media next", { locked = true })
bind("XF86AudioPrev", "dshell media prev", { locked = true })
