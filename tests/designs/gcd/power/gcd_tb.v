// Testbench for gcd power-activity VCD generation.
// Drives the valid/ready protocol with a deterministic LFSR stimulus so the
// committed VCD is reproducible. Dump scope is the `dut` instance only: cell
// internals land in child scopes whose leaf names never match DEF net names.
`timescale 1ns/1ps
module gcd_tb;

  reg        clk = 0;
  reg        reset = 1;
  reg  [31:0] req_msg = 0;
  reg        req_val = 0;
  wire       req_rdy;
  reg        resp_rdy = 0;
  wire [15:0] resp_msg;
  wire       resp_val;

  gcd dut (
    .clk(clk), .req_msg(req_msg), .req_rdy(req_rdy), .req_val(req_val),
    .reset(reset), .resp_msg(resp_msg), .resp_rdy(resp_rdy), .resp_val(resp_val)
  );

  always #5 clk = ~clk;  // 100 MHz

  // xorshift32 for deterministic stimulus
  reg [31:0] lfsr = 32'h9E3779B9;
  always @(posedge clk) begin
    lfsr <= {lfsr[30:0], lfsr[31] ^ lfsr[21] ^ lfsr[1] ^ lfsr[0]};
  end

  integer i;
  initial begin
    $dumpfile("gcd.vcd");
    $dumpvars(0, dut);
    repeat (4) @(posedge clk);
    reset <= 0;
    // ~400 request/response transactions
    for (i = 0; i < 400; i = i + 1) begin
      @(negedge clk);
      req_msg <= lfsr;
      req_val <= 1;
      @(posedge clk);
      while (!req_rdy) @(posedge clk);
      @(negedge clk);
      req_val <= 0;
      resp_rdy <= 1;
      @(posedge clk);
      while (!resp_val) @(posedge clk);
      @(negedge clk);
      resp_rdy <= 0;
      repeat (2) @(posedge clk);
    end
    repeat (20) @(posedge clk);
    $finish;
  end

  // Sanity: print a few responses so a broken sim is visible in the log.
  integer seen;
  initial seen = 0;
  always @(posedge clk) begin
    if (resp_val && resp_rdy && seen < 8) begin
      $display("resp[%0d] = %0d", seen, resp_msg);
      seen = seen + 1;
    end
  end

endmodule
