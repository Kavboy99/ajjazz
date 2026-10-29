// Retry the two async OLED/MLED/FLASH handlers that exceeded the first export budget.
// Static analysis only; no vendor executable or USB access.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class RetryUpdaterLargeFunctions extends GhidraScript {
    @Override public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) throw new IllegalArgumentException("Expected output directory");
        File out = new File(args[0]);
        if (!out.isDirectory()) throw new IOException("Missing output directory");
        DecompInterface decompiler = new DecompInterface();
        if (!decompiler.openProgram(currentProgram)) throw new IOException("Cannot open decompiler");
        try (PrintWriter code = new PrintWriter(new File(out, "dock-updater-large-handlers.c"), StandardCharsets.UTF_8);
             PrintWriter status = new PrintWriter(new File(out, "dock-updater-large-handlers.tsv"), StandardCharsets.UTF_8)) {
            code.println("/* Related updater; protocol reference only, not AJ159 dock firmware. */");
            status.println("entry\tbytes\tcompleted\terror");
            for (long entry : new long[]{0x00604560L, 0x00613770L}) {
                Function function = getFunctionAt(toAddr(entry));
                if (function == null) throw new IOException("Missing known handler at " + Long.toHexString(entry));
                println("Retrying handler " + function.getEntryPoint());
                DecompileResults result = decompiler.decompileFunction(function, 60, monitor);
                boolean ok = result.decompileCompleted() && result.getDecompiledFunction() != null;
                status.println(function.getEntryPoint() + "\t" + function.getBody().getNumAddresses() + "\t" + ok + "\t" + result.getErrorMessage().replace('\n', ' '));
                status.flush();
                if (ok) {
                    code.println("\n/* ENTRY " + function.getEntryPoint() + " */");
                    code.println(result.getDecompiledFunction().getC());
                    code.flush();
                }
                println("Handler " + function.getEntryPoint() + " complete=" + ok);
            }
        } finally { decompiler.dispose(); }
    }
}
