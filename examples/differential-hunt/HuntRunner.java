/** Prints demo.Probe.row(0), row(1), ... until "NOROW", escaped exactly like the decaf harness. */
public final class HuntRunner {
  static String esc(String s) {
    StringBuilder b = new StringBuilder();
    for (int i = 0; i < s.length(); i++) {
      char c = s.charAt(i);
      if (c >= 0x20 && c < 0x7f && c != '\\') b.append(c);
      else if (c == '\\') b.append("\\\\");
      else b.append(String.format("\\u%04X", (int) c));
    }
    return b.toString();
  }
  public static void main(String[] a) {
    for (int i = 0; ; i++) {
      String r;
      try { r = demo.Probe.row(i); } catch (Throwable t) { r = "ROWFAIL " + t; }
      if ("NOROW".equals(r)) break;
      System.out.println(esc(String.valueOf(r)));
    }
  }
}
