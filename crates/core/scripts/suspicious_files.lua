-- Looks for file NAMES that are typical of malware:
--   * words like "trojan", "rootkit", "worm", "keylogger", "ransomware"
--   * fake double extensions like "invoice.pdf.exe"
--   * Windows system program names (svchost.exe, ...) outside the Windows folder
--
-- This only checks names. It is NOT an antivirus: real malware can have any
-- name, and a match can be harmless. Check anything it finds with your antivirus.

automation {
    name = "Look for suspicious files",

    run = function(ctx)
        local folders = { "~/Downloads", "~/Desktop", "~/AppData/Local/Temp" }
        local found = 0

        for _, folder in ipairs(folders) do
            if fs.exists(folder) then
                local hits, complete = security.scan(folder)
                for _, hit in ipairs(hits) do
                    found = found + 1
                    log(hit.path .. "   ->   " .. hit.reason)
                end
                if not complete then
                    log("Scan of " .. folder .. " stopped at the time limit.")
                end
            end
        end

        if found > 0 then
            notify(found .. " suspicious file name(s) found. Check them with your antivirus.")
        else
            log("Nothing suspicious found.")
        end
    end
}
