((globalThis) => {
    const core = globalThis.Deno.core;

    function formatArgs(args) {
        return args
            .map((arg) => {
                if (typeof arg === "string") return arg;
                try {
                    return JSON.stringify(arg, null, 2);
                } catch {
                    return String(arg);
                }
            })
            .join(" ");
    }

    globalThis.console = {
        log: (...args) => {
            // The second argument to core.print is `is_err` (false for stdout).
            core.print(formatArgs(args) + "\n", false);
        },
        warn: (...args) => {
            core.print(`[WARN] ${formatArgs(args)}\n`, false);
        },
        error: (...args) => {
            // The second argument to core.print is `is_err` (true for stderr).
            core.print(`[ERROR] ${formatArgs(args)}\n`, true);
        },
    };
})(globalThis);
