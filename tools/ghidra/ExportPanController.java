// Export first signed controller functions only; preserve app/boot exports.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class ExportPanController extends GhidraScript {
    @Override public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) throw new IllegalArgumentException("Expected output directory");
        File out = new File(args[0]);
        DecompInterface decompiler = new DecompInterface();
        if (!decompiler.openProgram(currentProgram)) throw new IOException("Cannot open decompiler");
        int count = 0, completed = 0;
        long deadline = System.nanoTime() + 240_000_000_000L;
        try (PrintWriter code = new PrintWriter(new File(out, currentProgram.getName() + ".controller.c"), StandardCharsets.UTF_8);
             PrintWriter index = new PrintWriter(new File(out, currentProgram.getName() + ".controller.tsv"), StandardCharsets.UTF_8)) {
            code.println("/* Controller component reference only; physical dock compatibility unverified. */");
            index.println("entry\tname\tbytes\tdecompiled");
            FunctionIterator functions = currentProgram.getFunctionManager().getFunctions(true);
            while (functions.hasNext() && !monitor.isCancelled()) {
                Function function = functions.next();
                long entry = function.getEntryPoint().getOffset();
                if (!((entry >= 0x10200 && entry < 0x30000) ||
                      (entry >= 0x2000C800L && entry < 0x2000F328L))) continue;
                if (++count > 2000 || System.nanoTime() > deadline) break;
                DecompileResults result = decompiler.decompileFunction(function, 4, monitor);
                boolean ok = result.decompileCompleted() && result.getDecompiledFunction() != null;
                index.println(function.getEntryPoint() + "\t" + function.getName() + "\t" + function.getBody().getNumAddresses() + "\t" + ok);
                if (ok) {
                    code.println("\n/* ENTRY " + function.getEntryPoint() + " */");
                    code.println(result.getDecompiledFunction().getC());
                    completed++;
                }
            }
        } finally { decompiler.dispose(); }
        println("Controller functions exported: " + completed + "/" + count);
    }
}
