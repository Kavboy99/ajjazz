// Seed audited x86 frame prologues around unresolved dangle4k.rs references.
// Static reference analysis only, never run updater/device code.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class ExportUpdaterUsbBridge extends GhidraScript {
    @Override public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) throw new IllegalArgumentException("Expected output directory");
        File out = new File(args[0]);
        if (!out.isDirectory()) throw new IOException("Missing output directory");
        DecompInterface decompiler = new DecompInterface();
        if (!decompiler.openProgram(currentProgram)) throw new IOException("Cannot open decompiler");
        try (PrintWriter code = new PrintWriter(new File(out, "dock-updater-usb-bridge.c"), StandardCharsets.UTF_8);
             PrintWriter status = new PrintWriter(new File(out, "dock-updater-usb-bridge.tsv"), StandardCharsets.UTF_8)) {
            code.println("/* Related updater candidate functions; physical target compatibility unknown. */");
            status.println("entry\tbytes\tcompleted\terror");
            for (long entry : new long[]{0x728e20L, 0x729680L, 0x729de0L, 0x729f30L, 0x72a080L, 0x72ac30L, 0x72b800L}) {
                if (getByte(toAddr(entry)) != (byte)0x55 || getByte(toAddr(entry + 1)) != (byte)0x89 || getByte(toAddr(entry + 2)) != (byte)0xe5)
                    throw new IOException("Audited prologue mismatch");
                Function f = getFunctionAt(toAddr(entry));
                if (f == null) {
                    if (getFunctionContaining(toAddr(entry)) != null) throw new IOException("Candidate overlaps existing function");
                    disassemble(toAddr(entry));
                    f = createFunction(toAddr(entry), null);
                }
                if (f == null) throw new IOException("Cannot create function");
                println("USB bridge candidate " + f.getEntryPoint());
                DecompileResults result = decompiler.decompileFunction(f, 30, monitor);
                boolean ok = result.decompileCompleted() && result.getDecompiledFunction() != null;
                status.println(f.getEntryPoint() + "\t" + f.getBody().getNumAddresses() + "\t" + ok + "\t" + result.getErrorMessage().replace('\n', ' '));
                status.flush();
                if (ok) { code.println("\n/* ENTRY " + f.getEntryPoint() + " */"); code.println(result.getDecompiledFunction().getC()); code.flush(); }
            }
        } finally { decompiler.dispose(); }
    }
}
