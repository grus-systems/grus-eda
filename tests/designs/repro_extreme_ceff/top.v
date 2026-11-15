module top (a, b, y);
  input a;
  input b;
  output y;
  wire n1;
  sky130_fd_sc_hd__nor2_2 u1 (.A(a), .B(b), .Y(y));
endmodule
