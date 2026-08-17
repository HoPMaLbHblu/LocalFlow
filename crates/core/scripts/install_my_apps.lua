-- Setting up a new PC? List your apps once and install them all with one click.
-- Find an app's id with "winget search <name>" in a terminal, or on winget.run.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Install my apps",

    run = function(ctx)
        local apps = {
            "Mozilla.Firefox",
            "7zip.7zip",
            "VideoLAN.VLC",
            "Notepad++.Notepad++",
        }

        local installed, failed = 0, {}
        for _, id in ipairs(apps) do
            log("Installing " .. id .. "...")
            local ok, problem = pcall(packages.install, id)
            if ok then
                installed = installed + 1
            else
                -- Also happens when the app is already installed.
                log(id .. ": " .. tostring(problem))
                failed[#failed + 1] = id
            end
        end
        notify("Installed " .. installed .. " app(s)" .. (#failed > 0 and (", skipped: " .. table.concat(failed, ", ")) or ""))
    end
}
