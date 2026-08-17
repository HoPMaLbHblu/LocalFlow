-- Keeps a Windows service running: if it has stopped, starts it again.
-- The example watches the print spooler. Starting services usually needs
-- LocalFlow to run as administrator.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Keep a service running",

    run = function(ctx)
        local name = "Spooler"

        local s = service.status(name)
        if not s then
            log("There is no service called " .. name)
            return
        end
        if s.running then
            log(s.title .. " is running")
            return
        end
        service.start(name)
        notify(s.title .. " had stopped; started it again")
    end
}
