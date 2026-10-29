// Export source-pinned screen asset handlers; no hardware access.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class ExportScreenNative extends GhidraScript {
    @Override public void run() throws Exception {
        String[] args=getScriptArgs();
        DecompInterface d=new DecompInterface();
        d.openProgram(currentProgram);
        try(PrintWriter out=new PrintWriter(args[0],StandardCharsets.UTF_8)) {
            for(long a:new long[]{0x010139aaL,0x010139faL,0x01013a20L,0x01013a72L,
                                 0x01013afaL,0x01013b78L,0x01013c8eL,0x01013cceL}) {
                disassemble(toAddr(a));
                Function f=getFunctionAt(toAddr(a));
                if(f==null)f=createFunction(toAddr(a),null);
                if(f==null){out.println("Missing function "+Long.toHexString(a));continue;}
                DecompileResults r=d.decompileFunction(f,15,monitor);
                if(r.decompileCompleted())out.println("/* "+Long.toHexString(a)+" */\n"+r.getDecompiledFunction().getC());
            }
        }finally{d.dispose();}
    }
}
