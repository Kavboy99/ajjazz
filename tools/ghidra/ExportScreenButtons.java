// Source-pinned dock button/UART handlers; no hardware access.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import java.io.*;
import java.nio.charset.StandardCharsets;
public class ExportScreenButtons extends GhidraScript {
 public void run() throws Exception {
  DecompInterface d=new DecompInterface();d.openProgram(currentProgram);
  try(PrintWriter out=new PrintWriter(getScriptArgs()[0],StandardCharsets.UTF_8)) {
   for(long a:new long[]{0x01013d7cL,0x01013dd0L,0x01013decL,0x01013e40L,0x01013e60L,
    0x01013eb4L,0x01013f28L,0x01013f7cL,0x01014098L}) {
    disassemble(toAddr(a));Function f=getFunctionAt(toAddr(a));
    if(f==null)f=createFunction(toAddr(a),null);
    DecompileResults r=d.decompileFunction(f,20,monitor);
    if(r.decompileCompleted())out.println("/* "+Long.toHexString(a)+" */\n"+r.getDecompiledFunction().getC());
   }
  }finally{d.dispose();}
 }
}
