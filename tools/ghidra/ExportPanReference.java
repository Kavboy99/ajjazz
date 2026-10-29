// Export local reference decompilation and function index. No binary modifications.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class ExportPanReference extends GhidraScript {
    @Override public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) throw new IllegalArgumentException("Expected output directory");
        File out = new File(args[0]);
        if (!out.isDirectory() && !out.mkdirs()) throw new IOException("Cannot create output directory");
        DecompInterface decompiler = new DecompInterface();
        if (!decompiler.openProgram(currentProgram)) throw new IOException("Decompiler unavailable");
        int functions = 0, completed = 0;
        long deadline = System.nanoTime() + 300_000_000_000L;
        try (PrintWriter code = new PrintWriter(new File(out, currentProgram.getName() + ".c"), StandardCharsets.UTF_8);
             PrintWriter index = new PrintWriter(new File(out, currentProgram.getName() + ".functions.tsv"), StandardCharsets.UTF_8)) {
            code.println("/* Reference firmware decompilation; NOT an AJ159 APEX replacement or recovered source. */");
            index.println("entry\tname\tbytes\tdecompiled");
            FunctionIterator iterator = currentProgram.getFunctionManager().getFunctions(true);
            while (iterator.hasNext() && !monitor.isCancelled()) {
                Function function = iterator.next();
                long entry = function.getEntryPoint().getOffset();
                if (entry >= 0x10000 && entry < 0x30200 || entry >= 0x20000000L) continue;
                if (++functions > 4000 || System.nanoTime() > deadline) {
                    code.println("/* Export stopped at the configured resource limit. */");
                    break;
                }
                DecompileResults result = decompiler.decompileFunction(function, 5, monitor);
                boolean ok = result.decompileCompleted() && result.getDecompiledFunction() != null;
                index.println(function.getEntryPoint() + "\t" + function.getName() + "\t" + function.getBody().getNumAddresses() + "\t" + ok);
                if (ok) {
                    code.println("\n/* ENTRY " + function.getEntryPoint() + " */");
                    code.println(result.getDecompiledFunction().getC());
                    completed++;
                }
            }
        } finally { decompiler.dispose(); }
        println("Exported " + completed + " decompiled reference functions out of " + functions);
    }
}
