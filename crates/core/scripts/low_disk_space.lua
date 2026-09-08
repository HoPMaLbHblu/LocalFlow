-- Warns you when a disk is almost full.

automation {
    name = "Low disk space warning",

    run = function(ctx)
        local limit_gb = 10
        local gb = 1024 * 1024 * 1024

        for _, disk in ipairs(system.disks()) do
            local free = disk.free / gb
            log(string.format("%s  %.1f GB free of %.1f GB", disk.mount, free, disk.total / gb))

            if free < limit_gb and not disk.removable then
                notify(string.format("Disk %s has only %.1f GB free", disk.mount, free))
            end
        end
    end
}
