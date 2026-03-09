       IDENTIFICATION DIVISION.
       PROGRAM-ID. INTCALC.
       
       ENVIRONMENT DIVISION.
       INPUT-OUTPUT SECTION.
       FILE-CONTROL.
           SELECT INPUT-FILE ASSIGN TO 'input.txt'
               ORGANIZATION IS LINE SEQUENTIAL.
           SELECT OUTPUT-FILE ASSIGN TO 'output.txt'
               ORGANIZATION IS LINE SEQUENTIAL.
       
       DATA DIVISION.
       FILE SECTION.
       FD INPUT-FILE.
       01 INPUT-RECORD            PIC X(20).
       
       FD OUTPUT-FILE.
       01 OUTPUT-RECORD           PIC X(80).
       
       WORKING-STORAGE SECTION.
       01 WS-INPUT-AMOUNT         PIC 9(7)V99.
       01 WS-RESULT               PIC 9(7)V99.
       01 WS-RESULT-DISP          PIC ZZZ,ZZ9.99.
       
       PROCEDURE DIVISION.
       MAIN-LOGIC.
           OPEN INPUT INPUT-FILE
           OPEN OUTPUT OUTPUT-FILE
           
           READ INPUT-FILE INTO INPUT-RECORD
           END-READ
           
           MOVE SPACES TO OUTPUT-RECORD
           
           *> Parse input (assuming format like "10000.00")
           MOVE FUNCTION NUMVAL(INPUT-RECORD) TO WS-INPUT-AMOUNT
           
           DISPLAY "DEBUG: INPUT = " WS-INPUT-AMOUNT
           
           *> Simple multiplication: amount * 0.055
           MULTIPLY WS-INPUT-AMOUNT BY 0.055 GIVING WS-RESULT
           
           DISPLAY "DEBUG: RESULT = " WS-RESULT
           
           MOVE WS-RESULT TO WS-RESULT-DISP
           
           STRING "CALCULATED INTEREST: " 
                  DELIMITED BY SIZE
                  WS-RESULT-DISP
                  DELIMITED BY SIZE
                  INTO OUTPUT-RECORD
           END-STRING
           
           WRITE OUTPUT-RECORD
           
           CLOSE INPUT-FILE
           CLOSE OUTPUT-FILE
           
           STOP RUN.