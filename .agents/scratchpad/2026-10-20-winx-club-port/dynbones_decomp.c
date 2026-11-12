

// ===== FUN_800d4ea8 =====

void FUN_800d4ea8(void)

{
  int iVar1;
  int unaff_r2;
  int iVar2;
  undefined4 uVar3;
  int iVar4;
  int *piVar5;
  int iVar6;
  int *piVar7;
  int iVar8;
  int iVar9;
  int iVar10;
  int iVar11;
  int iVar12;
  undefined8 uVar13;
  
  uVar13 = FUN_800ac8d4();
  iVar2 = (int)((ulonglong)uVar13 >> 0x20);
  if ((*(int *)(iVar2 + 4) != 0) && (*(int *)(iVar2 + 0x1d0) == 0)) {
    piVar7 = *(int **)(*(int *)(iVar2 + 4) + 0x20);
    iVar6 = piVar7[2];
    uVar3 = FUN_800c5b08(0,*piVar7 * 0x24,0x20,0);
    *(undefined4 *)(iVar2 + 0x1d0) = uVar3;
    uVar3 = FUN_800c5b08(0,*piVar7 * 0xb4,0x20,0);
    *(undefined4 *)(iVar2 + 0x1dc) = uVar3;
    piVar5 = *(int **)(iVar2 + 0x1d0);
    iVar4 = (int)uVar13;
    if ((int)uVar13 == 0) {
      iVar4 = iVar2;
    }
    FUN_800d5020(iVar4);
    iVar8 = *(int *)(iVar4 + 0x1d8);
    iVar12 = *(int *)(iVar4 + 0x1d4);
    thunk_FUN_801244f4(iVar2 + 0x1e0);
    thunk_FUN_801244f4(iVar2 + 0x210);
    iVar11 = 0;
    iVar10 = 0;
    for (iVar9 = 0; iVar9 < *piVar7; iVar9 = iVar9 + 1) {
      iVar1 = *(int *)(unaff_r2 + -0x63a0);
      *piVar5 = *(int *)(iVar2 + 0x24) + iVar11;
      piVar5[1] = *(int *)(iVar2 + 0x28) + *(int *)(iVar6 + 0x94) * 0x30;
      piVar5[2] = iVar2 + 0x1e0;
      piVar5[3] = *(int *)(iVar6 + 0x90);
      piVar5[4] = iVar1;
      piVar5[5] = iVar4;
      piVar5[6] = iVar2 + 0x210;
      piVar5[7] = iVar12;
      piVar5[8] = iVar8;
      FUN_8010ec9c(*(int *)(iVar2 + 0x1dc) + iVar10);
      iVar11 = iVar11 + 0x30;
      iVar10 = iVar10 + 0xb4;
      iVar6 = iVar6 + 0xa0;
      piVar5 = piVar5 + 9;
    }
    *(undefined4 *)(iVar2 + 0x1cc) = 1;
  }
  FUN_800ac920();
  return;
}



// ===== FUN_800d5020 =====

void FUN_800d5020(int param_1)

{
  undefined4 uVar1;
  int iVar2;
  int iVar3;
  int iVar4;
  int iVar5;
  int *piVar6;
  
  if ((*(int *)(param_1 + 4) != 0) && (*(int *)(param_1 + 0x1d8) == 0)) {
    piVar6 = *(int **)(*(int *)(param_1 + 4) + 0x20);
    iVar4 = 0;
    iVar5 = piVar6[2];
    iVar2 = iVar5;
    for (iVar3 = 0; iVar3 < *piVar6; iVar3 = iVar3 + 1) {
      if (*(char *)(iVar2 + 0x2a) == '\x16') {
        iVar4 = iVar4 + 1;
      }
      iVar2 = iVar2 + 0xa0;
    }
    uVar1 = FUN_800c5b08(0,iVar4 << 2,0x20,0);
    *(undefined4 *)(param_1 + 0x1d8) = uVar1;
    iVar2 = 0;
    *(int *)(param_1 + 0x1d4) = iVar4;
    for (iVar3 = 0; iVar3 < *piVar6; iVar3 = iVar3 + 1) {
      if (*(char *)(iVar5 + 0x2a) == '\x16') {
        *(int *)(*(int *)(param_1 + 0x1d8) + iVar2) = iVar3;
        iVar2 = iVar2 + 4;
      }
      iVar5 = iVar5 + 0xa0;
    }
  }
  return;
}



// ===== FUN_800d52c0 =====

void FUN_800d52c0(undefined8 param_1,int param_2)

{
  int iVar1;
  int *piVar2;
  int iVar3;
  int iVar4;
  int iVar5;
  
  if (*(int *)(param_2 + 4) != 0) {
    piVar2 = *(int **)(*(int *)(param_2 + 4) + 0x20);
    iVar5 = 0;
    iVar4 = 0;
    iVar1 = piVar2[2];
    for (iVar3 = 0; iVar3 < *piVar2; iVar3 = iVar3 + 1) {
      if (*(int *)(iVar1 + 0x94) < 0) {
        FUN_80124520(*(int *)(param_2 + 0x24) + iVar5,*(int *)(param_2 + 0x28) + iVar5);
      }
      else {
        FUN_80124554(*(int *)(param_2 + 0x28) + *(int *)(iVar1 + 0x94) * 0x30,
                     *(int *)(param_2 + 0x24) + iVar5,*(int *)(param_2 + 0x28) + iVar5);
      }
      if (*(int *)(param_2 + 0x168) == 0) {
LAB_800d53c0:
        FUN_800d5114(param_1,param_2,iVar3);
      }
      else {
        if (*(char *)(param_2 + 0x5c) != '\0') {
          FUN_80124670(*(int *)(param_2 + 0x28) + iVar5,*(int *)(param_2 + 0x2c) + iVar5);
        }
        FUN_80124554(*(int *)(param_2 + 0x28) + iVar5,*(int *)(param_2 + 0x2c) + iVar5,
                     *(int *)(param_2 + 0x30) + iVar5);
        if (*(int *)(iVar1 + 0x2c) != 2) goto LAB_800d53c0;
        if ((*(int *)(param_2 + 0x20) == 0) || (*(int *)(*(int *)(param_2 + 0x20) + iVar4) == 0)) {
          FUN_800cec28(param_2,iVar1,*(int *)(param_2 + 0x3c) + iVar4);
          goto LAB_800d53c0;
        }
      }
      iVar5 = iVar5 + 0x30;
      iVar4 = iVar4 + 0x10;
      iVar1 = iVar1 + 0xa0;
    }
    FUN_800d5418(param_1,param_2);
    *(undefined4 *)(param_2 + 0x174) = 0;
  }
  return;
}



// ===== FUN_800d5114 =====

void FUN_800d5114(double param_1,int param_2,int param_3)

{
  int iVar1;
  undefined1 auStack_48 [60];
  
  if ((*(int *)(param_2 + 0x1cc) != 0) &&
     (iVar1 = *(int *)(*(int *)(*(int *)(param_2 + 4) + 0x20) + 8) + param_3 * 0xa0,
     *(char *)(iVar1 + 0x98) != '\0')) {
    *(float *)(*(int *)(param_2 + 0x1d0) + param_3 * 0x24 + 0x10) = (float)param_1;
    FUN_8010d430(auStack_48,*(int *)(param_2 + 0x1dc) + param_3 * 0xb4,
                 *(int *)(param_2 + 0x1d0) + param_3 * 0x24,*(undefined4 *)(iVar1 + 0x9c));
    param_3 = param_3 * 0x30;
    FUN_800c6570(*(int *)(param_2 + 0x24) + param_3,auStack_48);
    if (*(int *)(iVar1 + 0x94) < 0) {
      FUN_80124520(*(int *)(param_2 + 0x24) + param_3,*(int *)(param_2 + 0x28) + param_3);
    }
    else {
      FUN_80124554(*(int *)(param_2 + 0x28) + *(int *)(iVar1 + 0x94) * 0x30,
                   *(int *)(param_2 + 0x24) + param_3,*(int *)(param_2 + 0x28) + param_3);
    }
  }
  return;
}



// ===== FUN_800d5248 =====

void FUN_800d5248(int param_1,undefined4 param_2)

{
  int *piVar1;
  int iVar2;
  int iVar3;
  
  if ((*(int *)(param_1 + 4) != 0) && (*(int *)(param_1 + 0x1dc) != 0)) {
    piVar1 = *(int **)(*(int *)(param_1 + 4) + 0x20);
    iVar3 = 0;
    for (iVar2 = 0; iVar2 < *piVar1; iVar2 = iVar2 + 1) {
      FUN_8010ed1c(param_2,*(int *)(param_1 + 0x1dc) + iVar3);
      iVar3 = iVar3 + 0xb4;
    }
  }
  return;
}



// ===== FUN_800d5418 =====

/* WARNING: Removing unreachable block (ram,0x800d577c) */

void FUN_800d5418(double param_1,int param_2)

{
  float fVar1;
  float fVar2;
  float fVar3;
  int iVar4;
  int iVar5;
  int unaff_r2;
  int *piVar6;
  int iVar7;
  int iVar8;
  int iVar9;
  int iVar10;
  uint *puVar11;
  int iVar12;
  int iVar13;
  uint unaff_GQR0;
  double dVar14;
  
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf((byte)(unaff_GQR0 >> 8) & 0x3f);
  }
  if (*(int *)(param_2 + 0x160) == 0) {
    if ((*(char *)(param_2 + 0x5c) == '\0') &&
       (piVar6 = *(int **)(*(int *)(param_2 + 4) + 0x1c), piVar6 != (int *)0x0)) {
      iVar8 = piVar6[2];
      iVar9 = *(int *)(param_2 + 0x34);
      dVar14 = *(double *)(unaff_r2 + -0x62e8);
      fVar1 = *(float *)(unaff_r2 + -0x62f8);
      fVar2 = *(float *)(unaff_r2 + -0x6304);
      for (iVar7 = 0; iVar7 < *piVar6; iVar7 = iVar7 + 1) {
        if (*(int *)(iVar8 + 0x30) != 0) {
          fVar3 = fVar1 * (float)(param_1 / (double)*(float *)(param_2 + 0x1ac)) +
                  *(float *)(iVar9 + 0xc);
          *(float *)(iVar9 + 0xc) = fVar3;
          puVar11 = *(uint **)(iVar8 + 0x34);
          if ((float)((double)CONCAT44(0x43300000,puVar11[*(int *)(iVar8 + 0x30) + -1] ^ 0x80000000)
                     - dVar14) <= fVar3) {
            *(float *)(iVar9 + 0xc) =
                 fVar3 - (float)((double)CONCAT44(0x43300000,
                                                  puVar11[*(int *)(iVar8 + 0x30) + -1] ^ 0x80000000)
                                - dVar14);
          }
          iVar4 = *(int *)(iVar8 + 0x30);
          iVar10 = 0;
          if (0 < iVar4) {
            do {
              if (*(float *)(iVar9 + 0xc) <
                  (float)((double)CONCAT44(0x43300000,*puVar11 ^ 0x80000000) - dVar14)) {
                if (*(int *)(iVar9 + 0x14) == 0) {
                  *(int *)(iVar9 + 0x10) = iVar10;
                }
                break;
              }
              puVar11 = puVar11 + 1;
              iVar10 = iVar10 + 1;
              iVar4 = iVar4 + -1;
            } while (iVar4 != 0);
          }
        }
        iVar4 = *(int *)(iVar8 + 0x2c);
        if (iVar4 != 0) {
          if (*(int *)(iVar4 + 0x30) != 0) {
            fVar3 = fVar1 * (float)(param_1 / (double)*(float *)(param_2 + 0x1ac)) +
                    *(float *)(iVar9 + 0x38);
            *(float *)(iVar9 + 0x38) = fVar3;
            puVar11 = *(uint **)(iVar4 + 0x34);
            if ((float)((double)CONCAT44(0x43300000,
                                         puVar11[*(int *)(iVar4 + 0x30) + -1] ^ 0x80000000) - dVar14
                       ) <= fVar3) {
              *(float *)(iVar9 + 0x38) =
                   fVar3 - (float)((double)CONCAT44(0x43300000,
                                                    puVar11[*(int *)(iVar4 + 0x30) + -1] ^
                                                    0x80000000) - dVar14);
            }
            iVar10 = *(int *)(iVar4 + 0x30);
            iVar12 = 0;
            if (0 < iVar10) {
              do {
                if (*(float *)(iVar9 + 0x38) <
                    (float)((double)CONCAT44(0x43300000,*puVar11 ^ 0x80000000) - dVar14)) {
                  if (*(int *)(iVar9 + 0x40) == 0) {
                    *(int *)(iVar9 + 0x3c) = iVar12;
                  }
                  break;
                }
                puVar11 = puVar11 + 1;
                iVar12 = iVar12 + 1;
                iVar10 = iVar10 + -1;
              } while (iVar10 != 0);
            }
          }
          iVar10 = iVar9;
          for (iVar12 = 0; iVar12 < *(short *)(iVar8 + 0x2a); iVar12 = iVar12 + 1) {
            if (*(int *)(iVar4 + 0x30) != 0) {
              fVar3 = fVar1 * (float)(param_1 / (double)*(float *)(param_2 + 0x1ac)) +
                      *(float *)(iVar10 + 0x90);
              *(float *)(iVar10 + 0x90) = fVar3;
              puVar11 = *(uint **)(iVar4 + 0x34);
              if ((float)((double)CONCAT44(0x43300000,
                                           puVar11[*(int *)(iVar4 + 0x30) + -1] ^ 0x80000000) -
                         dVar14) <= fVar3) {
                *(float *)(iVar10 + 0x90) =
                     fVar3 - (float)((double)CONCAT44(0x43300000,
                                                      puVar11[*(int *)(iVar4 + 0x30) + -1] ^
                                                      0x80000000) - dVar14);
              }
              iVar5 = *(int *)(iVar4 + 0x30);
              iVar13 = 0;
              if (0 < iVar5) {
                do {
                  if (*(float *)(iVar10 + 0x90) <
                      (float)((double)CONCAT44(0x43300000,*puVar11 ^ 0x80000000) - dVar14)) {
                    if (*(int *)(iVar10 + 0x98) == 0) {
                      *(int *)(iVar10 + 0x94) = iVar13;
                    }
                    break;
                  }
                  puVar11 = puVar11 + 1;
                  iVar13 = iVar13 + 1;
                  iVar5 = iVar5 + -1;
                } while (iVar5 != 0);
              }
            }
            iVar10 = iVar10 + 0x2c;
            iVar4 = iVar4 + *(int *)(param_2 + 0xd0);
          }
        }
        if ((float)piVar6[1] < fVar2) {
          iVar8 = iVar8 + 0x38;
        }
        else {
          iVar8 = iVar8 + 0x50;
        }
        iVar9 = iVar9 + 0x134;
      }
    }
    fVar1 = *(float *)(unaff_r2 + -0x63a0);
    *(undefined1 *)(param_2 + 0x5c) = 0;
    if (((double)*(float *)(param_2 + 0x9c) != (double)fVar1) && (param_1 != (double)fVar1)) {
      param_1 = (double)((float)((double)*(float *)(param_2 + 0x9c) *
                                (double)*(float *)(param_2 + 0x1ac)) /
                        *(float *)(unaff_r2 + -0x62f8));
    }
    FUN_800d440c(param_1,param_2);
    if (*(int *)(param_2 + 0x1c4) != 0) {
      FUN_800d5798(param_1,param_2);
    }
    *(undefined4 *)(param_2 + 0x160) = 1;
  }
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-((byte)(unaff_GQR0 >> 0x18) & 0x3f));
  }
  return;
}



// ===== FUN_8010ec9c =====

void FUN_8010ec9c(int param_1)

{
  int unaff_r2;
  double dVar1;
  
  if (param_1 != 0) {
    dVar1 = (double)*(float *)(unaff_r2 + -0x5e80);
    FUN_800c6454(dVar1,dVar1,dVar1);
    dVar1 = (double)*(float *)(unaff_r2 + -0x5e80);
    FUN_800c6454(dVar1,dVar1,dVar1,param_1 + 0xc);
    *(undefined4 *)(param_1 + 0x18) = 0;
    *(undefined4 *)(param_1 + 0x1c) = 0;
    *(undefined4 *)(param_1 + 0x50) = 1;
    thunk_FUN_801244f4(param_1 + 0x20);
    thunk_FUN_801244f4(param_1 + 0x54);
    thunk_FUN_801244f4(param_1 + 0x84);
  }
  return;
}



// ===== FUN_8010ed1c =====

void FUN_8010ed1c(int param_1,int param_2)

{
  if (param_1 == 0) {
    return;
  }
  FUN_800c6570(param_2 + 0x84,param_1);
  return;
}



// ===== FUN_8010d430 =====

/* WARNING: Removing unreachable block (ram,0x8010d844) */

void FUN_8010d430(undefined4 param_1,undefined4 param_2,undefined4 *param_3,int *param_4)

{
  float fVar1;
  float fVar2;
  byte bVar3;
  int unaff_r2;
  int iVar4;
  int iVar5;
  int unaff_r13;
  uint unaff_GQR0;
  double dVar6;
  double dVar7;
  double dVar8;
  double in_f31;
  double in_ps31_1;
  undefined8 uVar9;
  float local_198;
  undefined1 auStack_194 [12];
  undefined1 auStack_188 [12];
  undefined1 auStack_17c [12];
  undefined1 auStack_170 [12];
  undefined1 auStack_164 [12];
  undefined1 auStack_158 [12];
  undefined1 auStack_14c [12];
  undefined1 auStack_140 [12];
  undefined1 auStack_134 [12];
  undefined1 auStack_128 [12];
  undefined1 auStack_11c [12];
  undefined1 auStack_110 [12];
  float local_104;
  float local_100;
  float local_fc;
  undefined1 auStack_f8 [12];
  float local_ec;
  float local_e8;
  float local_e4;
  undefined1 auStack_e0 [16];
  undefined1 auStack_d0 [12];
  undefined1 auStack_c4 [12];
  undefined1 auStack_b8 [48];
  undefined1 auStack_88 [48];
  undefined1 auStack_58 [80];
  undefined4 local_8;
  float fStack_4;
  
  bVar3 = (byte)unaff_GQR0 & 7;
  if ((unaff_GQR0 & 0x3f00) == 0) {
    dVar6 = 1.0;
  }
  else {
    dVar6 = (double)ldexpf((byte)(unaff_GQR0 >> 8) & 0x3f);
  }
  if (bVar3 == 4 || bVar3 == 6) {
    local_8 = (float)CONCAT13((char)(dVar6 * in_f31),
                              CONCAT12((char)(dVar6 * in_ps31_1),local_8._2_2_));
  }
  else if (bVar3 == 5 || bVar3 == 7) {
    local_8 = (float)CONCAT22((short)(dVar6 * in_f31),(short)(dVar6 * in_ps31_1));
  }
  else {
    local_8 = (float)in_f31;
    fStack_4 = (float)in_ps31_1;
  }
  uVar9 = FUN_800ac8ec();
  iVar5 = (int)uVar9;
  dVar6 = (double)*(float *)(unaff_r2 + -0x5e80);
  FUN_800c6454(dVar6,dVar6,dVar6,&local_ec);
  FUN_800c6580(auStack_58,param_3[1],*param_3);
  FUN_800c6580(auStack_88,param_3[2],auStack_58);
  FUN_800c6788(auStack_d0,auStack_88);
  if (*(int *)(iVar5 + 0x50) == 1) {
    FUN_800c6464(auStack_c4,&local_ec);
    FUN_800c6570(iVar5 + 0x54,*param_3);
  }
  else {
    FUN_800c6788(auStack_f8,iVar5 + 0x84);
    FUN_800c6788(auStack_c4,iVar5 + 0x20);
    FUN_800c6480(auStack_c4,auStack_c4,auStack_f8);
    thunk_FUN_801244f4(iVar5 + 0x84);
  }
  FUN_800c6494(auStack_e0,auStack_c4,auStack_d0);
  FUN_800c6464(&local_ec,auStack_e0);
  if (*param_4 == 1) {
    local_e8 = -(*(float *)(unaff_r2 + -0x5e7c) * *(float *)(unaff_r13 + -0x78a8) - local_e8);
  }
  local_fc = *(float *)(unaff_r13 + -0x78a8);
  local_104 = local_ec / local_fc;
  local_100 = local_e8 / local_fc;
  local_fc = local_e4 / local_fc;
  dVar6 = (double)thunk_FUN_80125298(&local_104);
  FUN_800c64b8(&local_ec,&local_ec);
  local_198 = *(float *)(unaff_r2 + -0x5e80);
  dVar7 = (double)local_198;
  FUN_800c6454(dVar7,dVar7,dVar7,auStack_128);
  FUN_8010e494(auStack_11c,auStack_128,iVar5,param_3);
  FUN_8010e30c(auStack_134,&local_ec,iVar5,param_3);
  FUN_8010e224(auStack_134,auStack_134,auStack_134,param_4);
  FUN_8010e494(auStack_140,auStack_134,iVar5,param_3);
  FUN_800c6464(&local_ec,auStack_140);
  FUN_8010d864(auStack_b8,&local_ec,iVar5,param_3);
  FUN_800c6464(auStack_110,&local_ec);
  iVar4 = FUN_8010da98(auStack_110,&local_198,&local_ec,auStack_b8,iVar5,param_3,param_4);
  if (iVar4 == 1) {
    FUN_800c6464(&local_ec,auStack_110);
    dVar6 = (double)(*(float *)(unaff_r2 + -0x5e78) * (float)(dVar6 * (double)ABS(local_198)));
  }
  FUN_800c6464(auStack_14c,iVar5);
  FUN_8010e30c(auStack_158,&local_ec,iVar5,param_3);
  FUN_8010e30c(auStack_170,auStack_14c,iVar5,param_3);
  if (*(int *)(iVar5 + 0x1c) == 0) {
    dVar8 = (double)((*(float *)(unaff_r2 + -0x5e74) * (float)param_3[4]) /
                    *(float *)(unaff_r2 + -0x5e70));
    dVar7 = dVar8;
    if ((*(char *)(param_4 + 0x14) == '\x01') && (*(char *)((int)param_4 + 0x52) == '\x01')) {
      dVar7 = (double)(((float)param_3[4] / *(float *)(unaff_r2 + -0x5e70)) *
                      (*(float *)(unaff_r2 + -0x5e6c) - (float)param_4[0x19]));
    }
    dVar7 = (double)((float)(dVar7 * dVar6) / *(float *)(unaff_r2 + -0x5e68));
    if ((double)*(float *)(unaff_r2 + -0x5e64) < dVar7) {
      dVar7 = (double)*(float *)(unaff_r2 + -0x5e64);
    }
    if ((*(char *)(param_4 + 0xc) == '\x01') && (*(char *)((int)param_4 + 0x32) == '\x01')) {
      dVar8 = (double)(((float)param_3[4] / *(float *)(unaff_r2 + -0x5e70)) *
                      (*(float *)(unaff_r2 + -0x5e6c) - (float)param_4[0x11]));
    }
    dVar6 = (double)((float)(dVar8 * dVar6) / *(float *)(unaff_r2 + -0x5e68));
    if ((double)*(float *)(unaff_r2 + -0x5e64) < dVar6) {
      dVar6 = (double)*(float *)(unaff_r2 + -0x5e64);
    }
    FUN_8010e5c8(dVar7,dVar6,auStack_164,auStack_170,auStack_158);
    FUN_8010e138(auStack_17c,auStack_164,auStack_170,param_4);
    if (iVar4 == 1) {
      FUN_8010e224(auStack_188,auStack_164,auStack_170,param_4);
      FUN_8010e494(iVar5,auStack_188,iVar5,param_3);
    }
    else {
      FUN_8010e224(auStack_188,auStack_17c,auStack_170,param_4);
      FUN_8010e494(iVar5,auStack_188,iVar5,param_3);
    }
  }
  else {
    fVar1 = *(float *)(unaff_r2 + -0x5e60);
    fVar2 = *(float *)(unaff_r2 + -0x5e70);
    *(int *)(iVar5 + 0x18) = *(int *)(iVar5 + 0x18) + 1;
    FUN_800c69b0((double)((fVar1 * (float)param_3[4]) / fVar2),iVar5,iVar5,iVar5 + 0xc);
    if (5 < *(int *)(iVar5 + 0x18)) {
      *(undefined4 *)(iVar5 + 0x1c) = 0;
    }
  }
  FUN_800c6464(auStack_194,iVar5);
  FUN_8010d864((int)((ulonglong)uVar9 >> 0x20),auStack_194,iVar5,param_3);
  *(undefined4 *)(iVar5 + 0x50) = 0;
  FUN_800c6570(iVar5 + 0x20,auStack_88);
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-((byte)(unaff_GQR0 >> 0x18) & 0x3f));
  }
  FUN_800ac938(1);
  return;
}



// ===== FUN_8010d864 =====

/* WARNING: Removing unreachable block (ram,0x8010da78) */

void FUN_8010d864(undefined4 param_1,undefined4 param_2,int param_3,int param_4)

{
  byte bVar1;
  int unaff_r2;
  float *pfVar2;
  undefined4 uVar3;
  uint unaff_GQR0;
  double in_f31;
  double dVar4;
  double dVar5;
  double in_ps31_1;
  undefined8 uVar6;
  undefined1 auStack_278 [12];
  undefined4 local_26c;
  float local_268;
  float local_264;
  float local_260;
  undefined4 local_25c;
  float local_258;
  undefined1 auStack_254 [12];
  undefined1 auStack_248 [48];
  undefined1 auStack_218 [48];
  undefined1 auStack_1e8 [48];
  undefined1 auStack_1b8 [48];
  undefined1 auStack_188 [48];
  float local_158;
  float local_154;
  float local_150;
  undefined1 auStack_14c [12];
  undefined1 auStack_140 [12];
  float local_134;
  float local_130;
  float local_12c;
  float local_128;
  float local_124;
  float local_120;
  undefined1 auStack_118 [48];
  undefined1 auStack_e8 [48];
  undefined1 auStack_b8 [48];
  undefined1 auStack_88 [48];
  undefined1 auStack_58 [12];
  undefined1 auStack_4c [12];
  undefined1 auStack_40 [12];
  float local_34;
  undefined4 local_8;
  float fStack_4;
  
  bVar1 = (byte)unaff_GQR0 & 7;
  if ((unaff_GQR0 & 0x3f00) == 0) {
    dVar4 = 1.0;
  }
  else {
    dVar4 = (double)ldexpf((byte)(unaff_GQR0 >> 8) & 0x3f);
  }
  if (bVar1 == 4 || bVar1 == 6) {
    local_8 = (float)CONCAT13((char)(dVar4 * in_f31),
                              CONCAT12((char)(dVar4 * in_ps31_1),local_8._2_2_));
  }
  else if (bVar1 == 5 || bVar1 == 7) {
    local_8 = (float)CONCAT22((short)(dVar4 * in_f31),(short)(dVar4 * in_ps31_1));
  }
  else {
    local_8 = (float)in_f31;
    fStack_4 = (float)in_ps31_1;
  }
  uVar6 = FUN_800ac8ec();
  pfVar2 = (float *)uVar6;
  uVar3 = *(undefined4 *)(param_4 + 4);
  dVar4 = (double)*(float *)(param_4 + 0xc);
  thunk_FUN_801244f4(auStack_88);
  FUN_800c6570(auStack_88,param_3 + 0x54);
  FUN_800c6580(auStack_e8,uVar3,auStack_88);
  FUN_800c6580(auStack_b8,*(undefined4 *)(param_4 + 8),auStack_e8);
  FUN_800c6594(auStack_118,auStack_b8);
  FUN_800c6788(&local_158,auStack_b8);
  FUN_800c6454(dVar4,(double)*(float *)(unaff_r2 + -0x5e80),(double)*(float *)(unaff_r2 + -0x5e80),
               auStack_254);
  FUN_800c67a4(auStack_14c,auStack_254,auStack_b8);
  FUN_800c6494(auStack_140,auStack_14c,&local_158);
  FUN_800c6464(auStack_58,&local_158);
  FUN_800c6464(auStack_4c,auStack_14c);
  FUN_800c64b8(auStack_40,auStack_140);
  local_34 = (float)dVar4;
  local_134 = (float)((double)*pfVar2 * dVar4 + (double)local_158);
  local_130 = (float)((double)pfVar2[1] * dVar4 + (double)local_154);
  local_12c = (float)((double)pfVar2[2] * dVar4 + (double)local_150);
  FUN_800c67a4(&local_128,&local_134,auStack_118);
  FUN_800c6464(&local_260,&local_128);
  FUN_800c64b8(&local_260,&local_260);
  local_25c = *(undefined4 *)(unaff_r2 + -0x5e80);
  FUN_800c64b8(&local_260,&local_260);
  dVar4 = (double)thunk_FUN_800b81b0(-(double)local_258,(double)local_260);
  dVar5 = (double)(float)dVar4;
  dVar4 = (double)thunk_FUN_800bb6d8((double)(local_128 * local_128 + local_120 * local_120));
  dVar4 = (double)thunk_FUN_800b81b0((double)local_124,(double)(float)dVar4);
  local_264 = (float)dVar4;
  local_26c = *(undefined4 *)(unaff_r2 + -0x5e80);
  local_268 = (float)dVar5;
  FUN_800c65f0(auStack_1b8);
  FUN_800c65e8((double)(float)dVar5,auStack_1e8);
  FUN_800c6580(auStack_218,auStack_1e8,auStack_1b8);
  FUN_800c6570(auStack_248,auStack_88);
  dVar4 = (double)*(float *)(unaff_r2 + -0x5e80);
  FUN_800c6454(dVar4,dVar4,dVar4,auStack_278);
  FUN_800c676c(auStack_248,auStack_278);
  FUN_800c6580(auStack_188,auStack_248,auStack_218);
  FUN_800c6788(auStack_278,auStack_88);
  FUN_800c676c(auStack_188,auStack_278);
  FUN_800c6570((int)((ulonglong)uVar6 >> 0x20),auStack_188);
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-((byte)(unaff_GQR0 >> 0x18) & 0x3f));
  }
  FUN_800ac938();
  return;
}



// ===== FUN_8010da98 =====

void FUN_8010da98(undefined4 param_1,undefined4 param_2,undefined4 param_3,undefined4 param_4,
                 undefined4 param_5,int param_6,int param_7)

{
  bool bVar1;
  uint uVar2;
  undefined4 uVar3;
  int iVar4;
  int iVar5;
  undefined1 *puVar6;
  int iVar7;
  int iVar8;
  int iVar9;
  double dVar10;
  undefined8 uVar11;
  undefined1 auStack_f8 [8];
  float local_f0;
  float local_ec;
  undefined1 auStack_e8 [12];
  undefined1 auStack_dc [12];
  undefined1 auStack_d0 [16];
  undefined1 auStack_c0 [48];
  undefined1 auStack_90 [48];
  undefined1 auStack_60 [96];
  
  uVar11 = FUN_800ac8d4();
  uVar3 = (undefined4)((ulonglong)uVar11 >> 0x20);
  iVar8 = *(int *)(param_6 + 0x1c) + -1;
  iVar7 = iVar8 * 4;
  iVar9 = 0;
  do {
    if (iVar8 < 0) goto LAB_8010dcac;
    bVar1 = false;
    iVar4 = FUN_800d90dc(*(undefined4 *)(param_6 + 0x14),
                         *(undefined4 *)(*(int *)(param_6 + 0x20) + iVar7));
    iVar5 = 0;
    for (uVar2 = (uint)*(byte *)(param_7 + 8); uVar2 != 0; uVar2 = uVar2 - 1) {
      if ((int)*(char *)(iVar4 + 0x2b) == (uint)*(byte *)(param_7 + iVar5 + 9)) {
        bVar1 = true;
        break;
      }
      iVar5 = iVar5 + 1;
    }
    if (!bVar1) {
      FUN_800c6570(auStack_60,param_4);
      iVar9 = FUN_8010dcd4(auStack_c0,(int)uVar11,param_3,auStack_60,param_5,param_6,param_7,iVar8);
      if (iVar9 == 1) {
        puVar6 = auStack_c0;
        iVar7 = 0;
        do {
          if (iVar7 == 0) {
            FUN_800c6464(auStack_d0,puVar6);
          }
          else {
            dVar10 = (double)FUN_800c6528(puVar6,param_3);
            local_f0 = (float)dVar10;
            dVar10 = (double)FUN_800c6528(auStack_d0,param_3);
            local_ec = (float)dVar10;
            if (local_f0 <= (float)dVar10) {
              FUN_800c6464(auStack_d0,puVar6);
            }
          }
          iVar7 = iVar7 + 1;
          puVar6 = puVar6 + 0xc;
        } while (iVar7 < 4);
        FUN_800c6494(auStack_e8,param_3,uVar3);
        FUN_800c64b8(auStack_e8,auStack_e8);
        iVar7 = *(int *)(param_6 + 0x1c);
        while (iVar7 = iVar7 + -1, -1 < iVar7) {
          if (iVar8 != iVar7) {
            FUN_8010d864(auStack_90,auStack_d0,param_5,param_6);
            FUN_800c6464(auStack_dc,auStack_d0);
            iVar5 = FUN_8010dcd4(auStack_c0,auStack_f8,auStack_d0,auStack_90,param_5,param_6,param_7
                                 ,iVar7);
            if (iVar5 == 1) {
              FUN_8010e8fc(auStack_dc,auStack_d0,auStack_e8,auStack_90,param_5,param_6,param_7,iVar7
                          );
              FUN_800c6464(auStack_d0,auStack_dc);
            }
          }
        }
LAB_8010dcac:
        FUN_800c6464(uVar3,auStack_d0);
        FUN_800ac920(iVar9);
        return;
      }
    }
    iVar7 = iVar7 + -4;
    iVar8 = iVar8 + -1;
  } while( true );
}



// ===== FUN_8010dcd4 =====

/* WARNING: Removing unreachable block (ram,0x8010e118) */
/* WARNING: Removing unreachable block (ram,0x8010e110) */

void FUN_8010dcd4(undefined4 param_1,undefined4 param_2,undefined4 param_3,undefined4 param_4,
                 undefined4 param_5,int param_6,undefined4 param_7,undefined1 *param_8)

{
  int iVar1;
  byte bVar2;
  int unaff_r2;
  int iVar3;
  int iVar4;
  int iVar5;
  int iVar6;
  int iVar7;
  undefined4 uVar8;
  undefined1 *puVar9;
  int *piVar10;
  undefined1 *puVar11;
  undefined1 *puVar12;
  undefined1 *puVar13;
  undefined1 *puVar14;
  undefined1 *puVar15;
  undefined1 *puVar16;
  uint unaff_GQR0;
  double dVar17;
  double dVar18;
  undefined8 uVar19;
  undefined1 auStack_a38 [12];
  undefined1 auStack_a2c [12];
  undefined1 auStack_a20 [12];
  undefined1 auStack_a14 [12];
  undefined1 auStack_a08 [12];
  undefined1 auStack_9fc [12];
  undefined1 auStack_9f0 [24];
  undefined1 auStack_9d8 [24];
  undefined1 auStack_9c0 [12];
  undefined1 auStack_9b4 [12];
  undefined1 auStack_9a8 [12];
  undefined1 auStack_99c [12];
  undefined1 auStack_990 [12];
  undefined1 auStack_984 [12];
  undefined1 auStack_978 [12];
  undefined1 auStack_96c [12];
  undefined1 auStack_960 [12];
  float local_954;
  undefined1 auStack_94c [12];
  undefined1 auStack_940 [12];
  undefined1 auStack_934 [36];
  undefined1 auStack_910 [48];
  undefined1 auStack_8e0 [48];
  undefined1 auStack_8b0 [48];
  undefined1 auStack_880 [12];
  undefined1 auStack_874 [12];
  undefined1 auStack_868 [12];
  float local_85c;
  undefined1 auStack_850 [48];
  undefined1 auStack_820 [48];
  undefined1 auStack_7f0 [4];
  undefined1 auStack_7ec [960];
  int local_42c;
  undefined1 auStack_428 [1064];
  
  bVar2 = (byte)(unaff_GQR0 >> 8);
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf(bVar2 & 0x3f);
  }
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf(bVar2 & 0x3f);
  }
  uVar19 = FUN_800ac8b8();
  iVar3 = (int)((ulonglong)uVar19 >> 0x20);
  uVar8 = *(undefined4 *)(param_6 + 4);
  iVar6 = 0;
  local_42c = 0;
  puVar9 = (undefined1 *)0x0;
  thunk_FUN_801244f4(auStack_910);
  dVar18 = (double)*(float *)(param_6 + 0xc);
  FUN_800c6580(auStack_8e0,uVar8,param_4);
  FUN_800c6580(auStack_8b0,*(undefined4 *)(param_6 + 8),auStack_8e0);
  FUN_800c6788(auStack_94c,auStack_8b0);
  FUN_800c6454(dVar18,(double)*(float *)(unaff_r2 + -0x5e80),(double)*(float *)(unaff_r2 + -0x5e80),
               auStack_9fc);
  FUN_800c67a4(auStack_940,auStack_9fc,auStack_8b0);
  FUN_800c6494(auStack_934,auStack_940,auStack_94c);
  FUN_800c6464(auStack_880,auStack_94c);
  FUN_800c6464(auStack_874,auStack_940);
  FUN_800c64b8(auStack_868,auStack_934);
  iVar1 = (int)param_8 * 4;
  dVar17 = (double)*(float *)(unaff_r2 + -0x5e80);
  local_85c = (float)((double)*(float *)(unaff_r2 + -0x5e5c) + dVar18);
  uVar8 = *(undefined4 *)(param_6 + 0x14);
  iVar4 = FUN_800d82c0(uVar8,&local_42c,auStack_880,auStack_910,
                       *(undefined4 *)(*(int *)(param_6 + 0x20) + iVar1));
  iVar7 = local_42c;
  if (iVar4 == 1) {
    puVar14 = auStack_428;
    piVar10 = &local_42c;
    puVar12 = puVar14;
    for (iVar4 = 0; iVar4 < iVar7; iVar4 = iVar4 + 1) {
      puVar11 = puVar12;
      if ((*(char *)(piVar10 + 0x1a) == '\0') &&
         (dVar18 = (double)FUN_800c6528(auStack_94c,puVar14), puVar11 = puVar9, dVar17 < dVar18)) {
        iVar6 = 1;
        *(int *)uVar19 = piVar10[0x1c];
        param_8 = puVar12;
        dVar17 = dVar18;
      }
      piVar10 = piVar10 + 0x1e;
      puVar14 = puVar14 + 0x78;
      puVar12 = puVar12 + 0x78;
      puVar9 = puVar11;
    }
  }
  if (iVar6 == 1) {
    FUN_800c6558(auStack_a08,param_8 + 0xc,auStack_868);
    dVar17 = (double)thunk_FUN_80125298(auStack_a08);
    if ((double)*(float *)(unaff_r2 + -0x5e58) < dVar17) {
      FUN_800c6608((double)*(float *)(unaff_r2 + -0x5e54),auStack_850,auStack_a08);
      FUN_800c6608((double)*(float *)(unaff_r2 + -0x5e50),auStack_820,auStack_a08);
      FUN_800c67a4(auStack_990,param_8 + 0xc,auStack_850);
      FUN_800c67a4(auStack_984,param_8 + 0xc,auStack_820);
      FUN_800c6480(auStack_9a8,param_8,auStack_990);
      FUN_800c6480(auStack_99c,param_8,auStack_984);
      if (puVar9 == (undefined1 *)0x0) {
        FUN_800c69b0((double)*(float *)(unaff_r2 + -0x5e4c),auStack_a14,param_8,auStack_874);
      }
      else {
        FUN_800c69b0((double)*(float *)(unaff_r2 + -0x5e4c),auStack_a14,param_8,puVar9);
      }
      FUN_800c6480(auStack_9c0,auStack_a14,auStack_990);
      FUN_800c6480(auStack_9b4,auStack_a14,auStack_984);
      puVar13 = auStack_9d8;
      puVar11 = auStack_9f0;
      dVar17 = (double)*(float *)(unaff_r2 + -0x5e48);
      puVar16 = auStack_9c0;
      puVar15 = auStack_9a8;
      iVar7 = 0;
      iVar4 = 0;
      puVar12 = puVar11;
      puVar14 = puVar13;
      do {
        FUN_800c6464(auStack_978,auStack_a14);
        FUN_800c6464(auStack_96c,puVar16);
        FUN_800c6494(auStack_960,auStack_96c,auStack_978);
        FUN_800c64b8(auStack_960,auStack_960);
        dVar18 = (double)FUN_800c64f8(auStack_96c,auStack_978);
        local_954 = (float)(dVar17 + dVar18);
        iVar5 = FUN_800d82c0(uVar8,auStack_7f0,auStack_978,auStack_910,
                             *(undefined4 *)(iVar1 + *(int *)(param_6 + 0x20)));
        if (iVar5 == 1) {
          FUN_800c6494(auStack_a38,puVar15,param_8);
          FUN_800c64b8(auStack_a38,auStack_a38);
          FUN_800c6494(auStack_a2c,auStack_7ec,param_8);
          thunk_FUN_801252dc(auStack_a38,auStack_a2c);
          FUN_800c64a8(auStack_a2c,auStack_a38);
          FUN_800c6480(puVar14,auStack_a2c,param_8);
        }
        else {
          FUN_800c6464(puVar14,param_8);
        }
        FUN_800c6494(puVar12,puVar13,auStack_94c);
        FUN_800c64b8(puVar12,puVar11);
        FUN_800c6464(iVar3 + iVar4,puVar11);
        FUN_800c6464(auStack_a20,puVar11);
        if (iVar5 == 0) {
          if (puVar9 != (undefined1 *)0x0) {
            FUN_8010e67c(auStack_a20,auStack_94c,param_3,param_8,puVar9,
                         *(undefined4 *)(iVar1 + *(int *)(param_6 + 0x20)),param_6);
          }
        }
        else {
          FUN_8010e67c(auStack_a20,auStack_94c,param_3,param_8,auStack_7ec,
                       *(undefined4 *)(iVar1 + *(int *)(param_6 + 0x20)),param_6);
        }
        FUN_800c6464(iVar3 + iVar4 + 0xc,auStack_a20);
        iVar7 = iVar7 + 1;
        puVar15 = puVar15 + 0xc;
        puVar14 = puVar14 + 0xc;
        puVar13 = puVar13 + 0xc;
        puVar12 = puVar12 + 0xc;
        puVar11 = puVar11 + 0xc;
        iVar4 = iVar4 + 0x18;
        puVar16 = puVar16 + 0xc;
      } while (iVar7 < 2);
    }
    else {
      iVar6 = 0;
    }
  }
  bVar2 = (byte)(unaff_GQR0 >> 0x18);
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-(bVar2 & 0x3f));
  }
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-(bVar2 & 0x3f));
  }
  FUN_800ac904(iVar6);
  return;
}



// ===== FUN_8010e8fc =====

/* WARNING: Removing unreachable block (ram,0x8010ec7c) */
/* WARNING: Removing unreachable block (ram,0x8010ec74) */

void FUN_8010e8fc(undefined4 param_1,undefined4 param_2,undefined4 param_3,undefined4 param_4,
                 undefined4 param_5,int param_6,undefined4 param_7,int param_8)

{
  byte bVar1;
  bool bVar2;
  int unaff_r2;
  undefined4 uVar3;
  int iVar4;
  int iVar5;
  undefined1 *puVar6;
  undefined1 *puVar7;
  undefined1 *unaff_r23;
  undefined4 uVar8;
  undefined1 *puVar9;
  undefined1 *puVar10;
  int *piVar11;
  uint unaff_GQR0;
  double dVar12;
  double dVar13;
  undefined1 auStack_9e8 [12];
  undefined1 auStack_9dc [12];
  undefined1 auStack_9d0 [12];
  undefined1 auStack_9c4 [12];
  undefined1 auStack_9b8 [12];
  undefined1 auStack_9ac [12];
  undefined1 auStack_9a0 [12];
  undefined1 auStack_994 [12];
  int local_988;
  int local_984;
  int local_980;
  undefined1 auStack_97c [12];
  undefined4 local_970;
  undefined4 local_96c;
  undefined4 local_968;
  undefined4 local_964;
  undefined4 local_960;
  undefined4 local_95c;
  undefined1 auStack_958 [12];
  float local_94c;
  undefined1 auStack_940 [48];
  undefined1 auStack_910 [12];
  undefined1 auStack_904 [12];
  undefined4 local_8f8;
  undefined4 local_8f4;
  undefined4 local_8f0;
  undefined4 local_8ec;
  undefined4 local_8e8;
  undefined4 local_8e4;
  undefined1 auStack_8d4 [12];
  undefined1 auStack_8c8 [12];
  undefined1 auStack_8bc [36];
  undefined1 auStack_898 [48];
  undefined1 auStack_868 [48];
  undefined1 auStack_838 [48];
  undefined1 auStack_808 [12];
  undefined1 auStack_7fc [12];
  undefined1 auStack_7f0 [12];
  float local_7e4;
  int local_7dc [241];
  int local_418;
  undefined1 auStack_414 [1044];
  
  bVar1 = (byte)(unaff_GQR0 >> 8);
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf(bVar1 & 0x3f);
  }
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf(bVar1 & 0x3f);
  }
  uVar3 = FUN_800ac8d0();
  uVar8 = *(undefined4 *)(param_6 + 4);
  bVar2 = false;
  local_418 = 0;
  puVar6 = (undefined1 *)0x0;
  thunk_FUN_801244f4(auStack_898);
  dVar13 = (double)*(float *)(param_6 + 0xc);
  FUN_800c6580(auStack_868,uVar8,param_4);
  FUN_800c6580(auStack_838,*(undefined4 *)(param_6 + 8),auStack_868);
  FUN_800c6788(auStack_8d4,auStack_838);
  FUN_800c6454(dVar13,(double)*(float *)(unaff_r2 + -0x5e80),(double)*(float *)(unaff_r2 + -0x5e80),
               auStack_994);
  FUN_800c67a4(auStack_8c8,auStack_994,auStack_838);
  FUN_800c6494(auStack_8bc,auStack_8c8,auStack_8d4);
  FUN_800c6464(auStack_808,auStack_8d4);
  FUN_800c6464(auStack_7fc,auStack_8c8);
  FUN_800c64b8(auStack_7f0,auStack_8bc);
  dVar12 = (double)*(float *)(unaff_r2 + -0x5e80);
  local_7e4 = (float)((double)*(float *)(unaff_r2 + -0x5e40) + dVar13);
  iVar4 = FUN_800d82c0(*(undefined4 *)(param_6 + 0x14),&local_418,auStack_808,auStack_898,
                       *(undefined4 *)(*(int *)(param_6 + 0x20) + param_8 * 4));
  iVar5 = local_418;
  if (iVar4 == 1) {
    puVar9 = auStack_414;
    piVar11 = &local_418;
    puVar10 = puVar9;
    for (iVar4 = 0; iVar4 < iVar5; iVar4 = iVar4 + 1) {
      puVar7 = puVar9;
      if ((*(char *)(piVar11 + 0x1a) == '\0') &&
         (dVar13 = (double)FUN_800c6528(auStack_8d4,puVar10), puVar7 = puVar6, dVar12 < dVar13)) {
        bVar2 = true;
        unaff_r23 = puVar9;
        dVar12 = dVar13;
      }
      piVar11 = piVar11 + 0x1e;
      puVar10 = puVar10 + 0x78;
      puVar9 = puVar9 + 0x78;
      puVar6 = puVar7;
    }
  }
  if (bVar2) {
    dVar12 = (double)*(float *)(unaff_r2 + -0x5e80);
    thunk_FUN_801244f4(auStack_940);
    FUN_800c6464(auStack_910,unaff_r23);
    if (puVar6 == (undefined1 *)0x0) {
      FUN_800c6464(auStack_904,auStack_8c8);
    }
    else {
      FUN_800c6464(auStack_904,puVar6);
    }
    FUN_800c6494(auStack_9a0,auStack_904,auStack_910);
    FUN_800c64a8((double)*(float *)(unaff_r2 + -0x5e4c),auStack_9ac,auStack_9a0);
    FUN_800c6480(&local_8f8,auStack_910,auStack_9ac);
    FUN_800c64a8((double)*(float *)(unaff_r2 + -0x5e40),auStack_9ac,param_3);
    FUN_800c6480(&local_8ec,&local_8f8,auStack_9ac);
    local_970 = local_8f8;
    local_96c = local_8f4;
    local_968 = local_8f0;
    local_964 = local_8ec;
    local_960 = local_8e8;
    local_95c = local_8e4;
    FUN_800c6494(auStack_958,&local_8ec,&local_8f8);
    FUN_800c64b8(auStack_958,auStack_958);
    dVar13 = (double)FUN_800c64f8(&local_8f8,&local_8ec);
    local_94c = (float)dVar13;
    iVar5 = FUN_800d82c0(*(undefined4 *)(param_6 + 0x14),local_7dc,&local_970,auStack_940,
                         *(undefined4 *)(*(int *)(param_6 + 0x20) + param_8 * 4));
    if (iVar5 == 1) {
      piVar11 = local_7dc;
      for (iVar5 = 0; iVar5 < local_7dc[0]; iVar5 = iVar5 + 1) {
        local_988 = piVar11[1];
        local_984 = piVar11[2];
        local_980 = piVar11[3];
        FUN_800c6480(auStack_97c,&local_988,unaff_r23 + 0xc);
        FUN_800c6494(auStack_9e8,auStack_97c,&local_988);
        FUN_800c64b8(auStack_9e8,auStack_9e8);
        FUN_800c6494(auStack_9dc,unaff_r23,&local_988);
        thunk_FUN_801252dc(auStack_9e8,auStack_9dc);
        FUN_800c64a8(auStack_9dc,auStack_9e8);
        FUN_800c6480(auStack_9b8,auStack_9dc,&local_988);
        dVar13 = (double)FUN_800c6528(auStack_9b8,param_3);
        if ((iVar5 == 0) || (dVar13 < dVar12)) {
          FUN_800c6464(auStack_9c4,auStack_9b8);
          dVar12 = dVar13;
        }
        piVar11 = piVar11 + 0x1e;
      }
      FUN_800c6494(auStack_9d0,auStack_9c4,auStack_8d4);
      FUN_800c64b8(auStack_9d0,auStack_9d0);
      FUN_800c6464(uVar3,auStack_9d0);
    }
  }
  bVar1 = (byte)(unaff_GQR0 >> 0x18);
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-(bVar1 & 0x3f));
  }
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-(bVar1 & 0x3f));
  }
  FUN_800ac91c();
  return;
}



// ===== FUN_8010e67c =====

/* WARNING: Removing unreachable block (ram,0x8010e8dc) */

void FUN_8010e67c(undefined4 param_1,undefined4 param_2,undefined4 param_3,int param_4,int param_5,
                 undefined4 param_6,int param_7)

{
  byte bVar1;
  int unaff_r2;
  int iVar2;
  int *piVar3;
  uint unaff_GQR0;
  double dVar4;
  double in_f31;
  double dVar5;
  double in_ps31_1;
  undefined8 uVar6;
  undefined1 auStack_528 [12];
  undefined1 auStack_51c [12];
  undefined1 auStack_510 [12];
  undefined1 auStack_504 [12];
  undefined1 auStack_4f8 [12];
  undefined1 auStack_4ec [12];
  undefined1 auStack_4e0 [12];
  undefined1 auStack_4d4 [12];
  int local_4c8;
  int local_4c4;
  int local_4c0;
  undefined1 auStack_4bc [12];
  undefined1 auStack_4b0 [12];
  undefined1 auStack_4a4 [12];
  undefined4 local_498;
  undefined4 local_494;
  undefined4 local_490;
  undefined4 local_48c;
  undefined4 local_488;
  undefined4 local_484;
  undefined1 auStack_480 [12];
  float local_474;
  undefined1 auStack_468 [48];
  undefined1 auStack_438 [12];
  undefined1 auStack_42c [12];
  undefined1 auStack_420 [12];
  undefined4 local_414;
  undefined4 local_410;
  undefined4 local_40c;
  undefined4 local_408;
  undefined4 local_404;
  undefined4 local_400;
  int local_3fc [253];
  undefined4 local_8;
  float fStack_4;
  
  bVar1 = (byte)unaff_GQR0 & 7;
  if ((unaff_GQR0 & 0x3f00) == 0) {
    dVar5 = 1.0;
  }
  else {
    dVar5 = (double)ldexpf((byte)(unaff_GQR0 >> 8) & 0x3f);
  }
  if (bVar1 == 4 || bVar1 == 6) {
    local_8 = (float)CONCAT13((char)(dVar5 * in_f31),
                              CONCAT12((char)(dVar5 * in_ps31_1),local_8._2_2_));
  }
  else if (bVar1 == 5 || bVar1 == 7) {
    local_8 = (float)CONCAT22((short)(dVar5 * in_f31),(short)(dVar5 * in_ps31_1));
  }
  else {
    local_8 = (float)in_f31;
    fStack_4 = (float)in_ps31_1;
  }
  uVar6 = FUN_800ac8e4();
  dVar5 = (double)*(float *)(unaff_r2 + -0x5e80);
  thunk_FUN_801244f4(auStack_468);
  FUN_800c6464(auStack_4b0,param_4 + 0xc);
  FUN_800c6464(auStack_4a4,param_5 + 0xc);
  FUN_800c6558(auStack_4ec,auStack_4b0,auStack_4a4);
  FUN_800c6464(auStack_438,param_4);
  FUN_800c6464(auStack_42c,param_5);
  FUN_800c6494(auStack_4d4,auStack_42c,auStack_438);
  FUN_800c64a8((double)*(float *)(unaff_r2 + -0x5e4c),auStack_4e0,auStack_4d4);
  FUN_800c6480(auStack_420,auStack_438,auStack_4e0);
  FUN_800c64a8((double)*(float *)(unaff_r2 + -0x5e40),auStack_4e0,auStack_4ec);
  FUN_800c6480(&local_414,auStack_420,auStack_4e0);
  FUN_800c64a8((double)*(float *)(unaff_r2 + -0x5e3c),auStack_4e0,auStack_4ec);
  FUN_800c6480(&local_408,auStack_420,auStack_4e0);
  local_498 = local_414;
  local_494 = local_410;
  local_490 = local_40c;
  local_48c = local_408;
  local_488 = local_404;
  local_484 = local_400;
  FUN_800c6494(auStack_480,&local_408,&local_414);
  FUN_800c64b8(auStack_480,auStack_480);
  dVar4 = (double)FUN_800c64f8(&local_414,&local_408);
  local_474 = (float)dVar4;
  iVar2 = FUN_800d82c0(*(undefined4 *)(param_7 + 0x14),local_3fc,&local_498,auStack_468,param_6);
  if (iVar2 == 1) {
    piVar3 = local_3fc;
    for (iVar2 = 0; iVar2 < local_3fc[0]; iVar2 = iVar2 + 1) {
      local_4c8 = piVar3[1];
      local_4c4 = piVar3[2];
      local_4c0 = piVar3[3];
      FUN_800c6480(auStack_4bc,&local_4c8,param_4 + 0xc);
      FUN_800c6494(auStack_528,auStack_4bc,&local_4c8);
      FUN_800c64b8(auStack_528,auStack_528);
      FUN_800c6494(auStack_51c,param_4,&local_4c8);
      thunk_FUN_801252dc(auStack_528,auStack_51c);
      FUN_800c64a8(auStack_51c,auStack_528);
      FUN_800c6480(auStack_4f8,auStack_51c,&local_4c8);
      dVar4 = (double)FUN_800c6528(auStack_4f8,param_3);
      if ((iVar2 == 0) || (dVar4 < dVar5)) {
        FUN_800c6464(auStack_504,auStack_4f8);
        dVar5 = dVar4;
      }
      piVar3 = piVar3 + 0x1e;
    }
    FUN_800c6494(auStack_510,auStack_504,(int)uVar6);
    FUN_800c64b8(auStack_510,auStack_510);
    FUN_800c6464((int)((ulonglong)uVar6 >> 0x20),auStack_510);
  }
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-((byte)(unaff_GQR0 >> 0x18) & 0x3f));
  }
  FUN_800ac930();
  return;
}



// ===== FUN_8010e138 =====

void FUN_8010e138(undefined4 param_1,int param_2,undefined4 param_3,int param_4)

{
  float fVar1;
  float fVar2;
  int unaff_r2;
  undefined1 auStack_28 [4];
  float local_24;
  float local_20;
  
  FUN_800c6464(auStack_28);
  if (*(char *)(param_4 + 0x50) == '\x01') {
    fVar1 = *(float *)(param_2 + 4);
    fVar2 = *(float *)(unaff_r2 + -0x5e44) * *(float *)(param_4 + 0x60) * ABS(fVar1);
    if (fVar1 <= *(float *)(unaff_r2 + -0x5e80)) {
      if (fVar1 < *(float *)(unaff_r2 + -0x5e80)) {
        local_24 = fVar1 + fVar2;
      }
    }
    else {
      local_24 = fVar1 - fVar2;
    }
  }
  if (*(char *)(param_4 + 0x30) == '\x01') {
    fVar1 = *(float *)(param_2 + 8);
    fVar2 = *(float *)(unaff_r2 + -0x5e44) * *(float *)(param_4 + 0x40) * ABS(fVar1);
    if (fVar1 <= *(float *)(unaff_r2 + -0x5e80)) {
      if (fVar1 < *(float *)(unaff_r2 + -0x5e80)) {
        local_20 = fVar1 + fVar2;
      }
    }
    else {
      local_20 = fVar1 - fVar2;
    }
  }
  FUN_800c6464(param_1,auStack_28);
  return;
}



// ===== FUN_8010e224 =====

void FUN_8010e224(undefined4 param_1,undefined4 param_2,undefined4 param_3,int param_4)

{
  int unaff_r2;
  undefined1 auStack_18 [4];
  float local_14;
  float local_10;
  
  FUN_800c6464(auStack_18);
  if (*(char *)(param_4 + 0x50) == '\x01') {
    if (*(char *)(param_4 + 0x51) == '\x01') {
      if (local_14 < *(float *)(param_4 + 0x54)) {
        local_14 = *(float *)(param_4 + 0x54);
      }
      if (*(float *)(param_4 + 0x58) < local_14) {
        local_14 = *(float *)(param_4 + 0x58);
      }
    }
  }
  else {
    local_14 = *(float *)(unaff_r2 + -0x5e80);
  }
  if (*(char *)(param_4 + 0x30) == '\x01') {
    if (*(char *)(param_4 + 0x31) == '\x01') {
      if (local_10 < -*(float *)(param_4 + 0x38)) {
        local_10 = -*(float *)(param_4 + 0x38);
      }
      if (-*(float *)(param_4 + 0x34) < local_10) {
        local_10 = -*(float *)(param_4 + 0x34);
      }
    }
  }
  else {
    local_10 = *(float *)(unaff_r2 + -0x5e80);
  }
  FUN_800c6464(param_1,auStack_18);
  return;
}



// ===== FUN_8010e30c =====

/* WARNING: Removing unreachable block (ram,0x8010e470) */

void FUN_8010e30c(undefined4 *param_1,float *param_2,int param_3,int param_4)

{
  undefined4 uVar1;
  int unaff_r2;
  uint unaff_GQR0;
  double dVar2;
  double dVar3;
  float local_138;
  undefined4 local_134;
  float local_130;
  float local_12c;
  undefined4 local_128;
  undefined4 local_124;
  float local_120;
  float local_11c;
  float local_118;
  undefined1 auStack_114 [12];
  undefined1 auStack_108 [12];
  float local_fc;
  float local_f8;
  float local_f4;
  float local_f0;
  float local_ec;
  float local_e8;
  undefined1 auStack_e0 [48];
  undefined1 auStack_b0 [48];
  undefined1 auStack_80 [48];
  undefined1 auStack_50 [12];
  undefined1 auStack_44 [12];
  undefined1 auStack_38 [12];
  float local_2c;
  
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf((byte)(unaff_GQR0 >> 8) & 0x3f);
  }
  dVar2 = (double)*(float *)(param_4 + 0xc);
  FUN_800c6580(auStack_b0,*(undefined4 *)(param_4 + 4),param_3 + 0x54);
  FUN_800c6580(auStack_80,*(undefined4 *)(param_4 + 8),auStack_b0);
  FUN_800c6594(auStack_e0,auStack_80);
  FUN_800c6788(&local_120,auStack_80);
  local_128 = *(undefined4 *)(unaff_r2 + -0x5e80);
  local_12c = (float)dVar2;
  local_124 = local_128;
  FUN_800c67a4(auStack_114,&local_12c,auStack_80);
  FUN_800c6464(auStack_50,&local_120);
  FUN_800c6464(auStack_44,auStack_114);
  FUN_800c64b8(auStack_38,auStack_108);
  local_2c = (float)dVar2;
  local_fc = (float)((double)*param_2 * dVar2 + (double)local_120);
  local_f8 = (float)((double)param_2[1] * dVar2 + (double)local_11c);
  local_f4 = (float)((double)param_2[2] * dVar2 + (double)local_118);
  FUN_800c67a4(&local_f0,&local_fc,auStack_e0);
  FUN_800c6464(&local_138,&local_f0);
  FUN_800c64b8(&local_138,&local_138);
  local_134 = *(undefined4 *)(unaff_r2 + -0x5e80);
  FUN_800c64b8(&local_138,&local_138);
  dVar2 = (double)thunk_FUN_800b81b0(-(double)local_130,(double)local_138);
  dVar3 = (double)(float)dVar2;
  dVar2 = (double)thunk_FUN_800bb6d8((double)(local_f0 * local_f0 + local_e8 * local_e8));
  dVar2 = (double)thunk_FUN_800b81b0((double)local_ec,(double)(float)dVar2);
  uVar1 = *(undefined4 *)(unaff_r2 + -0x5e80);
  param_1[1] = (float)dVar3;
  *param_1 = uVar1;
  param_1[2] = (float)dVar2;
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-((byte)(unaff_GQR0 >> 0x18) & 0x3f));
  }
  return;
}



// ===== FUN_8010e494 =====

void FUN_8010e494(undefined4 param_1,int param_2,int param_3,int param_4)

{
  int unaff_r2;
  double dVar1;
  undefined4 local_1b8;
  undefined4 local_1b4;
  undefined4 local_1b0;
  undefined1 auStack_1ac [12];
  undefined1 auStack_1a0 [12];
  undefined1 auStack_194 [12];
  undefined1 auStack_188 [40];
  undefined1 auStack_160 [48];
  undefined1 auStack_130 [48];
  undefined1 auStack_100 [48];
  undefined1 auStack_d0 [48];
  undefined1 auStack_a0 [48];
  undefined1 auStack_70 [48];
  undefined1 auStack_40 [48];
  
  FUN_800c65f0((double)*(float *)(param_2 + 8),auStack_70);
  FUN_800c65e8((double)*(float *)(param_2 + 4),auStack_a0);
  FUN_800c6580(auStack_d0,auStack_a0,auStack_70);
  FUN_800c6570(auStack_100,param_3 + 0x54);
  dVar1 = (double)*(float *)(unaff_r2 + -0x5e80);
  FUN_800c6454(dVar1,dVar1,dVar1,auStack_1ac);
  FUN_800c676c(auStack_100,auStack_1ac);
  FUN_800c6580(auStack_40,auStack_100,auStack_d0);
  FUN_800c6788(auStack_1ac,param_3 + 0x54);
  FUN_800c676c(auStack_40,auStack_1ac);
  FUN_800c6580(auStack_160,*(undefined4 *)(param_4 + 4),auStack_40);
  FUN_800c6580(auStack_130,*(undefined4 *)(param_4 + 8),auStack_160);
  FUN_800c6788(auStack_1a0,auStack_130);
  local_1b8 = *(undefined4 *)(param_4 + 0xc);
  local_1b4 = *(undefined4 *)(unaff_r2 + -0x5e80);
  local_1b0 = local_1b4;
  FUN_800c67a4(auStack_194,&local_1b8,auStack_130);
  FUN_800c6494(auStack_188,auStack_194,auStack_1a0);
  FUN_800c64b8(param_1,auStack_188);
  return;
}



// ===== FUN_8010e5c8 =====

/* WARNING: Removing unreachable block (ram,0x8010e660) */

void FUN_8010e5c8(undefined8 param_1,undefined8 param_2,undefined4 param_3,int param_4,int param_5)

{
  int unaff_r2;
  uint unaff_GQR0;
  undefined1 auStack_68 [12];
  undefined1 auStack_5c [12];
  undefined4 local_50;
  undefined4 local_4c;
  undefined4 local_48;
  undefined4 local_44;
  undefined4 local_40;
  undefined4 local_3c;
  undefined4 local_38;
  undefined4 local_34;
  undefined4 local_30;
  undefined4 local_2c;
  undefined4 local_28;
  undefined4 local_24;
  
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf((byte)(unaff_GQR0 >> 8) & 0x3f);
  }
  local_50 = *(undefined4 *)(unaff_r2 + -0x5e80);
  local_48 = *(undefined4 *)(param_4 + 8);
  local_3c = *(undefined4 *)(param_5 + 8);
  local_34 = *(undefined4 *)(param_4 + 4);
  local_28 = *(undefined4 *)(param_5 + 4);
  local_4c = local_50;
  local_44 = local_50;
  local_40 = local_50;
  local_38 = local_50;
  local_30 = local_50;
  local_2c = local_50;
  local_24 = local_50;
  FUN_800c69b0(auStack_68,&local_50,&local_44);
  FUN_800c69b0(param_2,auStack_5c,&local_38,&local_2c);
  FUN_800c6480(param_3,auStack_68,auStack_5c);
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-((byte)(unaff_GQR0 >> 0x18) & 0x3f));
  }
  return;
}



// ===== FUN_800d82c0 =====

undefined4 FUN_800d82c0(int param_1,undefined4 *param_2,undefined4 *param_3,int param_4,int param_5)

{
  int iVar1;
  int *piVar2;
  int iVar3;
  int iVar4;
  int iVar5;
  undefined4 uVar6;
  undefined1 auStack_b8 [48];
  undefined1 auStack_88 [48];
  undefined4 local_58;
  undefined4 local_54;
  undefined4 local_50;
  undefined4 local_4c;
  undefined4 local_48;
  undefined4 local_44;
  undefined4 local_40;
  undefined4 local_3c;
  undefined4 local_38;
  undefined4 local_34;
  undefined4 local_30;
  
  uVar6 = 0;
  piVar2 = *(int **)(*(int *)(param_1 + 4) + 0x20);
  iVar5 = piVar2[2];
  *param_2 = 0;
  local_58 = *param_3;
  local_54 = param_3[1];
  local_50 = param_3[2];
  local_4c = param_3[3];
  local_48 = param_3[4];
  local_44 = param_3[5];
  local_40 = param_3[6];
  local_3c = param_3[7];
  local_38 = param_3[8];
  local_34 = param_3[9];
  local_30 = param_3[10];
  if (param_5 == 0) {
    param_5 = 0;
    iVar3 = *piVar2;
  }
  else {
    iVar3 = param_5 + 1;
    iVar5 = iVar5 + param_5 * 0xa0;
  }
  iVar4 = param_5 * 0x30;
  for (; param_5 < iVar3; param_5 = param_5 + 1) {
    if ((*(short *)(iVar5 + 0x9a) != 0) && (*(char *)(iVar5 + 0x28) == '\0')) {
      if (param_4 == 0) {
        FUN_800c6570(auStack_b8,*(int *)(param_1 + 0x28) + iVar4);
      }
      else {
        FUN_800c6580(auStack_b8,param_4,*(int *)(param_1 + 0x28) + iVar4);
      }
      FUN_800c6594(auStack_88,auStack_b8);
      FUN_800c67a4(&local_58,param_3,auStack_88);
      FUN_800c67a4(&local_4c,param_3 + 3,auStack_88);
      FUN_80124d70(auStack_88,param_3 + 6,&local_40);
      iVar1 = FUN_800d8450(param_1,param_2,&local_58,iVar5,auStack_b8,param_5);
      if (iVar1 != 0) {
        uVar6 = 1;
      }
    }
    iVar4 = iVar4 + 0x30;
    iVar5 = iVar5 + 0xa0;
  }
  return uVar6;
}



// ===== FUN_800c69b0 =====

/* WARNING: Removing unreachable block (ram,0x800c6a04) */

void FUN_800c69b0(undefined8 param_1,undefined4 param_2,undefined4 param_3,undefined4 param_4)

{
  uint unaff_GQR0;
  undefined1 auStack_28 [16];
  
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf((byte)(unaff_GQR0 >> 8) & 0x3f);
  }
  FUN_801251fc(param_4,param_3,auStack_28);
  FUN_80125220(param_1,auStack_28,auStack_28);
  FUN_801251d8(auStack_28,param_3,param_2);
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-((byte)(unaff_GQR0 >> 0x18) & 0x3f));
  }
  return;
}



// ===== FUN_800c6608 =====

/* WARNING: Removing unreachable block (ram,0x800c66dc) */
/* WARNING: Removing unreachable block (ram,0x800c66d4) */

void FUN_800c6608(undefined8 param_1,float *param_2,float *param_3)

{
  byte bVar1;
  int unaff_r2;
  uint unaff_GQR0;
  double dVar2;
  double dVar3;
  double dVar4;
  double dVar5;
  double dVar6;
  double dVar7;
  
  bVar1 = (byte)(unaff_GQR0 >> 8);
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf(bVar1 & 0x3f);
  }
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf(bVar1 & 0x3f);
  }
  dVar2 = (double)FUN_800bb574();
  dVar7 = (double)(float)dVar2;
  dVar2 = (double)FUN_800bb168(param_1);
  dVar6 = (double)(float)dVar2;
  FUN_801244f4(param_2);
  dVar4 = (double)*param_3;
  dVar3 = (double)param_3[1];
  dVar2 = (double)param_3[2];
  dVar5 = (double)(float)((double)*(float *)(unaff_r2 + -0x6440) - dVar6);
  *param_2 = (float)(dVar5 * (double)(float)(dVar4 * dVar4) + dVar6);
  param_2[1] = (float)(dVar5 * (double)(float)(dVar3 * dVar4) + (double)(float)(dVar2 * dVar7));
  param_2[5] = (float)(dVar5 * (double)(float)(dVar3 * dVar3) + dVar6);
  param_2[4] = (float)(dVar5 * (double)(float)(dVar3 * dVar4) - (double)(float)(dVar2 * dVar7));
  param_2[10] = (float)(dVar5 * (double)(float)(dVar2 * dVar2) + dVar6);
  param_2[2] = (float)(dVar5 * (double)(float)(dVar2 * dVar4) - (double)(float)(dVar3 * dVar7));
  param_2[6] = (float)(dVar5 * (double)(float)(dVar2 * dVar3) + (double)(float)(dVar4 * dVar7));
  param_2[8] = (float)(dVar5 * (double)(float)(dVar2 * dVar4) + (double)(float)(dVar3 * dVar7));
  param_2[9] = (float)(dVar5 * (double)(float)(dVar2 * dVar3) - (double)(float)(dVar4 * dVar7));
  bVar1 = (byte)(unaff_GQR0 >> 0x18);
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-(bVar1 & 0x3f));
  }
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-(bVar1 & 0x3f));
  }
  return;
}



// ===== FUN_800c65e8 =====

void FUN_800c65e8(undefined4 param_1)

{
  FUN_80124768(param_1,0x79);
  return;
}



// ===== FUN_800c65f0 =====

void FUN_800c65f0(undefined4 param_1)

{
  FUN_80124768(param_1,0x7a);
  return;
}



// ===== FUN_800c67a4 =====

void FUN_800c67a4(undefined4 param_1,undefined4 param_2,undefined4 param_3)

{
  FUN_80124d1c(param_3,param_2,param_1);
  return;
}



// ===== FUN_800c676c =====

void FUN_800c676c(int param_1,undefined4 *param_2)

{
  undefined4 uVar1;
  undefined4 uVar2;
  
  uVar1 = param_2[1];
  uVar2 = param_2[2];
  *(undefined4 *)(param_1 + 0xc) = *param_2;
  *(undefined4 *)(param_1 + 0x1c) = uVar1;
  *(undefined4 *)(param_1 + 0x2c) = uVar2;
  return;
}



// ===== FUN_800c6580 =====

void FUN_800c6580(undefined4 param_1,undefined4 param_2,undefined4 param_3)

{
  FUN_80124554(param_2,param_3,param_1);
  return;
}



// ===== FUN_800c6788 =====

void FUN_800c6788(undefined4 *param_1,int param_2)

{
  undefined4 uVar1;
  undefined4 uVar2;
  
  uVar1 = *(undefined4 *)(param_2 + 0x1c);
  uVar2 = *(undefined4 *)(param_2 + 0x2c);
  *param_1 = *(undefined4 *)(param_2 + 0xc);
  param_1[1] = uVar1;
  param_1[2] = uVar2;
  return;
}



// ===== FUN_800c6494 =====

void FUN_800c6494(undefined4 param_1,undefined4 param_2,undefined4 param_3)

{
  FUN_801251fc(param_2,param_3,param_1);
  return;
}



// ===== FUN_800c6480 =====

void FUN_800c6480(undefined4 param_1,undefined4 param_2,undefined4 param_3)

{
  FUN_801251d8(param_2,param_3,param_1);
  return;
}



// ===== FUN_800c64a8 =====

void FUN_800c64a8(undefined4 param_1,undefined4 param_2)

{
  FUN_80125220(param_2,param_1);
  return;
}



// ===== FUN_800c64b8 =====

void FUN_800c64b8(undefined4 param_1,float *param_2)

{
  float fVar1;
  int unaff_r2;
  
  fVar1 = *(float *)(unaff_r2 + -0x643c);
  if (((fVar1 == *param_2) && (fVar1 == param_2[1])) && (fVar1 == param_2[2])) {
    return;
  }
  FUN_8012523c(param_2,param_1);
  return;
}



// ===== FUN_800c6528 =====

void FUN_800c6528(undefined4 param_1,undefined4 param_2)

{
  undefined1 auStack_18 [24];
  
  FUN_801251fc(param_1,param_2,auStack_18);
  FUN_80125280(auStack_18);
  return;
}



// ===== FUN_800c64f8 =====

void FUN_800c64f8(undefined4 param_1,undefined4 param_2)

{
  undefined1 auStack_18 [24];
  
  FUN_801251fc(param_1,param_2,auStack_18);
  FUN_80125298(auStack_18);
  return;
}



// ===== FUN_800a6d3c =====

void FUN_800a6d3c(int *param_1,int param_2)

{
  float fVar1;
  float fVar2;
  float fVar3;
  int iVar4;
  int unaff_r2;
  int *piVar5;
  int unaff_r13;
  int iVar6;
  
  fVar1 = *(float *)(unaff_r2 + -0x6bc8);
  iVar4 = *param_1;
  param_1[0xb] = (int)fVar1;
  param_1[0xc] = (int)fVar1;
  param_1[0xd] = (int)fVar1;
  param_1[0xe] = (int)fVar1;
  if (iVar4 == 2) {
    fVar2 = (float)param_1[2] * *(float *)(unaff_r2 + -0x6bc0);
    fVar3 = fVar1;
    if (((float)param_1[1] < fVar2) && (fVar3 = (float)param_1[1] / fVar2, fVar1 < fVar3)) {
      fVar3 = fVar1;
    }
    fVar1 = *(float *)(unaff_r2 + -0x6bbc);
    param_1[0xd] = (int)fVar3;
    param_1[0xc] = (int)fVar3;
    param_1[0xb] = (int)fVar3;
    if (fVar1 * (float)param_1[2] <= (float)param_1[1]) {
      FUN_8003fb58(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x10],6);
      FUN_8003fb58(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x11],6);
    }
    else {
      FUN_8003fb58(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x10],5);
      FUN_8003fb58(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x11],5);
    }
    iVar4 = FUN_8003fa94(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x11]);
    if (iVar4 != 0) {
      FUN_8003f744((double)*(float *)(unaff_r2 + -0x6bc8),param_1[0x1a],
                   param_1[param_1[0xf] * 5 + 0x10],1,1);
      FUN_8003f744((double)*(float *)(unaff_r2 + -0x6bc8),param_1[0x1a],
                   param_1[param_1[0xf] * 5 + 0x11],1,1);
      iVar4 = *(int *)(unaff_r2 + -0x6bd8);
      *param_1 = 3;
      param_1[1] = iVar4;
    }
  }
  else if (iVar4 < 2) {
    if (iVar4 == 0) {
      iVar4 = 0;
      iVar6 = 2;
      piVar5 = param_1;
      do {
        if (((piVar5[0x13] & 2U) != 0) && (piVar5[0x12] == 0)) {
          param_1[0xf] = iVar4;
          if (param_1[iVar4 * 5 + 0x14] != -1) {
            FUN_800379e0(param_1[0x1d]);
          }
          iVar4 = *(int *)(unaff_r2 + -0x6bd8);
          *param_1 = 1;
          param_1[1] = iVar4;
          break;
        }
        piVar5 = piVar5 + 5;
        iVar4 = iVar4 + 1;
        iVar6 = iVar6 + -1;
      } while (iVar6 != 0);
    }
    else if (-1 < iVar4) {
      fVar2 = *(float *)(unaff_r2 + -0x6bc4);
      fVar1 = fVar1 - (float)param_1[1] / fVar2;
      param_1[0xd] = (int)fVar1;
      param_1[0xc] = (int)fVar1;
      param_1[0xb] = (int)fVar1;
      if (fVar2 < (float)param_1[1]) {
        FUN_8003f744(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x10],0,0);
        FUN_8003f744((double)*(float *)(unaff_r2 + -0x6bc8),param_1[0x1a],
                     param_1[param_1[0xf] * 5 + 0x11],0,0);
        FUN_8003f85c(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x10],0);
        FUN_8003f85c(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x11],0);
        FUN_8003f974(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x10],0);
        FUN_8003f974(param_1[0x1a],param_1[param_1[0xf] * 5 + 0x11],0);
        fVar1 = *(float *)(unaff_r2 + -0x6bd8);
        param_1[param_1[0xf] * 5 + 0x12] = 2;
        iVar4 = FUN_800fafac((double)fVar1,*(undefined4 *)(unaff_r13 + -0x734c),0,4,0x23);
        param_1[8] = iVar4;
        FUN_8003d1d8(param_1[0x1b],0);
        FUN_800406d8(param_1[0x1c],param_1[param_1[0xf] * 5 + 0x10],param_1[param_1[0xf] * 5 + 0x11]
                    );
        FUN_800406d8(param_1[0x1c],param_1[param_1[0xf] * 5 + 0x11],param_1[param_1[0xf] * 5 + 0x10]
                    );
        iVar4 = *(int *)(unaff_r2 + -0x6bd8);
        *param_1 = 2;
        param_1[1] = iVar4;
      }
    }
  }
  else if (iVar4 == 4) {
    fVar2 = *(float *)(unaff_r2 + -0x6bc4);
    param_1[0xd] = (int)fVar1;
    param_1[0xc] = (int)fVar1;
    param_1[0xb] = (int)fVar1;
    if (fVar2 < (float)param_1[1]) {
      iVar4 = *(int *)(unaff_r2 + -0x6bd8);
      *param_1 = 0;
      param_1[1] = iVar4;
    }
  }
  else if (iVar4 < 4) {
    param_1[0xd] = (int)fVar1;
    param_1[0xc] = (int)fVar1;
    param_1[0xb] = (int)fVar1;
  }
  if (param_2 == 0) {
    iVar4 = *(int *)(*(int *)(unaff_r13 + -0x72a8) + 0x1d4);
    param_1[1] = (int)((float)param_1[1] +
                      *(float *)(unaff_r2 + -0x6bc8) /
                      (*(float *)(iVar4 + 0x50) /
                      (float)((double)CONCAT44(0x43300000,*(int *)(iVar4 + 0x54) + 1U ^ 0x80000000)
                             - *(double *)(unaff_r2 + -0x6bb8))));
  }
  return;
}



// ===== FUN_800a7154 =====

void FUN_800a7154(int *param_1,undefined4 param_2,undefined4 param_3)

{
  int iVar1;
  int unaff_r2;
  int unaff_r13;
  undefined4 local_68;
  undefined4 local_64;
  undefined4 local_60;
  undefined4 local_5c;
  undefined4 local_58;
  undefined4 local_54;
  undefined1 auStack_50 [12];
  int local_44;
  float local_34;
  int local_24;
  undefined4 local_20;
  uint uStack_1c;
  
  local_68 = DAT_801e1cb8;
  local_64 = DAT_801e1cbc;
  local_60 = DAT_801e1cc0;
  local_5c = DAT_801e1cc4;
  local_58 = DAT_801e1cc8;
  local_54 = DAT_801e1ccc;
  iVar1 = *param_1;
  if (iVar1 == 3) {
    if (param_1[8] != -1) {
      local_20 = 0x43300000;
      iVar1 = *(int *)(*(int *)(unaff_r13 + -0x72a8) + 0x1d4);
      uStack_1c = *(int *)(iVar1 + 0x54) + 1U ^ 0x80000000;
      FUN_800fb13c((double)(*(float *)(unaff_r2 + -0x6bc4) *
                           (*(float *)(iVar1 + 0x50) /
                           (float)((double)CONCAT44(0x43300000,uStack_1c) -
                                  *(double *)(unaff_r2 + -0x6bb8)))),
                   *(undefined4 *)(unaff_r13 + -0x734c),param_1[8],1);
      param_1[8] = -1;
    }
  }
  else {
    if (iVar1 < 3) {
      if (iVar1 < 2) {
        if (-1 < iVar1) {
          FUN_800c6464(param_1 + 3,&local_68);
        }
      }
      else {
        FUN_8009b8b4((double)((float)param_1[1] / (float)param_1[2]),param_1 + 3,&local_68,&local_5c
                    );
        if (param_1[8] != -1) {
          thunk_FUN_801244f4(auStack_50);
          local_44 = param_1[3];
          local_34 = *(float *)(unaff_r2 + -0x6bb0) + (float)param_1[4];
          local_24 = param_1[5];
          if (param_1[8] != -1) {
            FUN_800fb1b4(*(undefined4 *)(unaff_r13 + -0x734c),param_1[8],auStack_50);
          }
        }
      }
      goto LAB_800a72c4;
    }
    if (4 < iVar1) goto LAB_800a72c4;
  }
  FUN_800c6464(param_1 + 3,&local_5c);
LAB_800a72c4:
  FUN_800c6a70((double)(float)param_1[6],(double)(float)param_1[7],param_1 + 9,param_1 + 3,param_2,
               param_3);
  if (*(float *)(unaff_r2 + -0x6bd8) <= (float)param_1[10]) {
    if ((float)param_1[7] < (float)param_1[10]) {
      param_1[10] = param_1[7];
    }
  }
  else {
    param_1[10] = (int)*(float *)(unaff_r2 + -0x6bd8);
  }
  return;
}



// ===== FUN_800a73e0 =====

void FUN_800a73e0(undefined4 param_1,undefined4 param_2,undefined4 param_3,undefined4 param_4)

{
  bool bVar1;
  int unaff_r2;
  int *piVar2;
  undefined4 uVar3;
  undefined4 uVar4;
  undefined4 uVar5;
  int iVar6;
  undefined8 uVar7;
  
  uVar7 = FUN_800ac8e8();
  piVar2 = (int *)((ulonglong)uVar7 >> 0x20);
  iVar6 = (int)uVar7;
  bVar1 = true;
  if ((*piVar2 != 2) && (*piVar2 != 3)) {
    bVar1 = false;
  }
  if (bVar1) {
    if (piVar2[iVar6 * 5 + 0x12] == 2) {
      if (*(float *)(unaff_r2 + -0x6bd8) < (float)piVar2[10]) {
        uVar3 = FUN_800ac7c4();
        uVar4 = FUN_800ac7c4((double)(float)piVar2[6]);
        FUN_8014b0ac(0,0,uVar4,uVar3);
        FUN_8003ea00(piVar2[0x1a],piVar2[iVar6 * 5 + 0x10],param_3,param_4,1);
        FUN_8003ea00(piVar2[0x1a],piVar2[iVar6 * 5 + 0x10],param_3,param_4,2);
        FUN_8003eaf8(piVar2[0x1a],piVar2[iVar6 * 5 + 0x10],param_3,param_4,1);
        FUN_8003eaf8(piVar2[0x1a],piVar2[iVar6 * 5 + 0x10],param_3,param_4,2);
      }
      if ((float)piVar2[10] < (float)piVar2[7]) {
        uVar3 = FUN_800ac7c4((double)((float)piVar2[7] - (float)piVar2[10]));
        uVar4 = FUN_800ac7c4((double)(float)piVar2[6]);
        uVar5 = FUN_800ac7c4((double)(float)piVar2[10]);
        FUN_8014b0ac(0,uVar5,uVar4,uVar3);
        FUN_8003ea00(piVar2[0x1a],piVar2[iVar6 * 5 + 0x11],param_3,param_4,1);
        FUN_8003ea00(piVar2[0x1a],piVar2[iVar6 * 5 + 0x11],param_3,param_4,2);
        FUN_8003eaf8(piVar2[0x1a],piVar2[iVar6 * 5 + 0x11],param_3,param_4,1);
        FUN_8003eaf8(piVar2[0x1a],piVar2[iVar6 * 5 + 0x11],param_3,param_4,2);
      }
      uVar3 = FUN_800ac7c4((double)(float)piVar2[7]);
      uVar4 = FUN_800ac7c4((double)(float)piVar2[6]);
      FUN_8014b0ac(0,0,uVar4,uVar3);
    }
  }
  FUN_800ac934();
  return;
}



// ===== FUN_800a7324 =====

void FUN_800a7324(int *param_1,undefined4 param_2,int param_3)

{
  bool bVar1;
  int iVar2;
  int *piVar3;
  undefined1 auStack_48 [56];
  
  bVar1 = true;
  if ((*param_1 != 2) && (*param_1 != 3)) {
    bVar1 = false;
  }
  if ((bVar1) && (param_1[param_3 * 5 + 0x12] == 2)) {
    thunk_FUN_801244f4(auStack_48);
    piVar3 = param_1 + param_3 * 5;
    iVar2 = 0;
    do {
      FUN_8003fb90(param_1[0x1a],piVar3[0x10],auStack_48);
      FUN_8003e928(param_1[0x1a],piVar3[0x10],param_2);
      iVar2 = iVar2 + 1;
      piVar3 = piVar3 + 1;
    } while (iVar2 < 2);
  }
  return;
}



// ===== FUN_800a7650 =====

void FUN_800a7650(int param_1,int param_2)

{
  uint uVar1;
  
  if (1 < param_2) {
    return;
  }
  param_1 = param_1 + param_2 * 0x14;
  uVar1 = *(uint *)(param_1 + 0x4c);
  if ((uVar1 & 1) == 0) {
    return;
  }
  *(uint *)(param_1 + 0x4c) = uVar1 | 2;
  return;
}



// ===== FUN_800a7678 =====

void FUN_800a7678(undefined4 *param_1)

{
  undefined4 uVar1;
  int unaff_r2;
  
  if (param_1[0x12] == 2) {
    param_1[0x12] = 4;
  }
  if (param_1[0x17] == 2) {
    param_1[0x17] = 4;
  }
  if (param_1[0x1b] != 0) {
    FUN_8003d224();
  }
  uVar1 = *(undefined4 *)(unaff_r2 + -0x6bd8);
  *param_1 = 4;
  param_1[1] = uVar1;
  return;
}



// ===== FUN_8003149c =====

/* WARNING: Removing unreachable block (ram,0x80031ea8) */

void FUN_8003149c(void)

{
  byte bVar1;
  bool bVar2;
  undefined4 uVar3;
  uint uVar4;
  int unaff_r2;
  int iVar5;
  int iVar6;
  int iVar7;
  int *piVar8;
  int *piVar9;
  int unaff_r13;
  int iVar10;
  int iVar11;
  int iVar12;
  uint unaff_GQR0;
  double dVar13;
  double in_f31;
  double in_ps31_1;
  undefined8 uVar14;
  undefined1 auStack_1f0 [8];
  int local_1e8;
  int local_1e4;
  undefined4 local_1e0;
  undefined4 local_1dc;
  undefined4 local_1d8;
  undefined4 local_1d4;
  undefined4 local_1d0;
  undefined4 local_1cc;
  undefined4 local_1c8;
  undefined4 local_1c4;
  undefined4 local_1c0;
  undefined4 local_1bc;
  undefined4 local_1b8;
  undefined4 local_1b4;
  undefined4 local_1b0;
  undefined4 local_1ac;
  undefined4 local_1a8;
  undefined4 local_1a4;
  undefined4 local_1a0;
  undefined4 local_19c;
  undefined4 local_198;
  undefined4 local_194;
  undefined4 local_190;
  int local_188;
  float local_184 [7];
  undefined1 auStack_168 [48];
  undefined1 auStack_138 [48];
  undefined1 auStack_108 [48];
  undefined1 auStack_d8 [64];
  undefined1 auStack_98 [48];
  undefined1 auStack_68 [28];
  float local_4c;
  undefined4 local_38;
  uint uStack_34;
  undefined4 local_8;
  float fStack_4;
  
  bVar1 = (byte)unaff_GQR0 & 7;
  if ((unaff_GQR0 & 0x3f00) == 0) {
    dVar13 = 1.0;
  }
  else {
    dVar13 = (double)ldexpf((byte)(unaff_GQR0 >> 8) & 0x3f);
  }
  if (bVar1 == 4 || bVar1 == 6) {
    local_8 = (float)CONCAT13((char)(dVar13 * in_f31),
                              CONCAT12((char)(dVar13 * in_ps31_1),local_8._2_2_));
  }
  else if (bVar1 == 5 || bVar1 == 7) {
    local_8 = (float)CONCAT22((short)(dVar13 * in_f31),(short)(dVar13 * in_ps31_1));
  }
  else {
    local_8 = (float)in_f31;
    fStack_4 = (float)in_ps31_1;
  }
  uVar14 = FUN_800ac8e0();
  iVar5 = (int)((ulonglong)uVar14 >> 0x20);
  iVar12 = (int)uVar14;
  FUN_80146ef8(1,8);
  FUN_80004104(auStack_1f0,0,4);
  FUN_800c2aa8(*(undefined4 *)(*(int *)(unaff_r13 + -0x72a8) + 0x1d4),auStack_1f0);
  if (iVar12 == 0) {
    iVar6 = FUN_8001ba10(*(undefined4 *)(unaff_r13 + -0x735c));
    if (iVar6 != 0) {
      FUN_8001b9b4(*(undefined4 *)(unaff_r13 + -0x735c),0);
    }
  }
  else {
    iVar6 = FUN_8001ba10(*(undefined4 *)(unaff_r13 + -0x735c));
    if (iVar6 == 0) {
      FUN_8001b9b4(*(undefined4 *)(unaff_r13 + -0x735c),1);
    }
  }
  if (*(int *)(iVar5 + 0x28c) != 0) {
    FUN_8001baa8(*(undefined4 *)(unaff_r13 + -0x735c));
    FUN_8001ca6c(*(undefined4 *)(unaff_r13 + -0x7348));
    FUN_8001cdc4(*(undefined4 *)(unaff_r13 + -0x7348),iVar12);
  }
  FUN_800307f0(iVar5,iVar12);
  if ((*(int *)(iVar5 + 0x80) != 3) && (*(int *)(iVar5 + 0x80) != 6)) {
    iVar6 = *(int *)(*(int *)(iVar5 + 700) + 0x60);
    if ((iVar6 == 1) || (iVar6 - 3U < 3)) {
      bVar2 = true;
    }
    else {
      bVar2 = false;
    }
    if ((!bVar2) && (iVar12 == 0)) {
      iVar6 = *(int *)(*(int *)(unaff_r13 + -0x7348) + 0x400770);
      dVar13 = (double)FUN_8001caec();
      uStack_34 = iVar6 >> 0xc ^ 0x80000000;
      local_38 = 0x43300000;
      if (dVar13 <= (double)(*(float *)(unaff_r2 + -0x7d0c) *
                            (float)((double)CONCAT44(0x43300000,uStack_34) -
                                   *(double *)(unaff_r2 + -0x7d28)))) {
        iVar10 = 0;
        iVar11 = 0;
        iVar6 = iVar5;
        do {
          iVar7 = *(int *)(unaff_r13 + -0x7348) + iVar11;
          if ((((*(int *)(iVar7 + 0x3a2bc) != 0) && (*(int *)(iVar7 + 0xdac10) == 1000)) &&
              (*(int *)(iVar6 + 0xe0) != -1)) && (*(int *)(iVar6 + 0x114) != -1)) {
            FUN_800a7650(iVar5 + 0x2dc,iVar10);
          }
          iVar10 = iVar10 + 1;
          iVar6 = iVar6 + 0x38;
          iVar11 = iVar11 + 0xa0ab8;
        } while (iVar10 < 5);
      }
    }
  }
  FUN_800a6d3c(iVar5 + 0x2dc,iVar12);
  uVar3 = 1;
  if ((*(int *)(iVar5 + 0x2dc) != 2) && (*(int *)(iVar5 + 0x2dc) != 3)) {
    uVar3 = 0;
  }
  uVar4 = countLeadingZeros(uVar3);
  iVar6 = 1;
  *(uint *)(*(int *)(iVar5 + 0x2c8) + 0x2b58) = uVar4 >> 5;
  if ((*(int *)(iVar5 + 0x2dc) != 2) && (*(int *)(iVar5 + 0x2dc) != 3)) {
    iVar6 = 0;
  }
  *(uint *)(*(int *)(iVar5 + 0x2cc) + 0x94) = (uint)-iVar6 >> 0x1f;
  for (iVar6 = 0; iVar6 < *(int *)(iVar5 + 0x234); iVar6 = iVar6 + 1) {
    uVar3 = 1;
    if ((*(int *)(iVar5 + 0x2dc) != 2) && (*(int *)(iVar5 + 0x2dc) != 3)) {
      uVar3 = 0;
    }
    uVar4 = countLeadingZeros(uVar3);
    FUN_8003ef0c(*(undefined4 *)(iVar5 + 0x2c4),iVar6,uVar4 >> 5);
  }
  FUN_80004000(*(int *)(iVar5 + 0x2c8) + 0x2b48,iVar5 + 0x308,0x10);
  FUN_80004000(*(int *)(iVar5 + 0x2cc) + 0x6c,iVar5 + 0x308,0x10);
  iVar6 = *(int *)(iVar5 + 0x2d8);
  if (iVar6 != 0) {
    (**(code **)(*(int *)(iVar6 + 0x270) + 0x20))(iVar6,iVar12);
  }
  FUN_800406b0(*(undefined4 *)(iVar5 + 0x2d0),iVar12);
  iVar6 = FUN_800407e8(*(undefined4 *)(iVar5 + 0x2d0),*(undefined4 *)(iVar5 + 0x2a0),&local_198);
  if (iVar6 != 0) {
    local_1d4 = local_198;
    local_1d0 = local_194;
    local_1cc = local_190;
    FUN_800c6464(*(int *)(iVar5 + 700) + 0x54,&local_1d4);
  }
  if (iVar12 == 0) {
    FUN_8003b4e4(*(undefined4 *)(iVar5 + 700),0);
  }
  else {
    iVar6 = *(int *)(iVar5 + 700);
    if ((*(int *)(iVar6 + 0x60) == 1) || (*(int *)(iVar6 + 0x60) - 3U < 3)) {
      bVar2 = true;
    }
    else {
      bVar2 = false;
    }
    if (bVar2) {
      FUN_8003b4e4(iVar6,0);
    }
    else {
      FUN_8003b4e4(iVar6,1);
    }
  }
  FUN_8003c224(*(undefined4 *)(iVar5 + 700),auStack_98);
  FUN_8003c2ac(*(undefined4 *)(iVar5 + 700),auStack_d8);
  FUN_8003c240(&local_1e0,*(undefined4 *)(iVar5 + 700));
  local_1a4 = local_1e0;
  local_1a0 = local_1dc;
  local_19c = local_1d8;
  uVar14 = FUN_8003c2e0(*(undefined4 *)(iVar5 + 700));
  FUN_8003c278(*(undefined4 *)(iVar5 + 700));
  FUN_800a7154(iVar5 + 0x2dc,auStack_98,auStack_d8);
  FUN_80040f4c(uVar14,*(undefined4 *)(iVar5 + 0x2c0));
  FUN_80040a14(*(undefined4 *)(iVar5 + 0x2c0),auStack_98);
  FUN_800c6570(auStack_138,*(int *)(iVar5 + 0x2c0) + 0x4a8);
  FUN_80036668(*(undefined4 *)(iVar5 + 0x2c8));
  FUN_800362a4(*(undefined4 *)(iVar5 + 0x2c8),iVar12);
  FUN_800364f0(*(undefined4 *)(iVar5 + 0x2c8),auStack_98,&local_1a4);
  FUN_800378b0(*(undefined4 *)(iVar5 + 0x2cc),iVar12);
  (**(code **)(*(int *)(*(int *)(iVar5 + 0x2cc) + 100) + 0x2c))
            (*(int *)(iVar5 + 0x2cc),auStack_98,&local_1a4);
  FUN_8003676c(*(undefined4 *)(iVar5 + 0x2c8),auStack_98,&local_1a4);
  FUN_80037914(*(undefined4 *)(iVar5 + 0x2cc),auStack_98,&local_1a4);
  for (iVar6 = 0; iVar6 < *(int *)(iVar5 + 0xd8); iVar6 = iVar6 + 1) {
    FUN_800a7324(iVar5 + 0x2dc,iVar12,iVar6);
  }
  *(undefined4 *)(*(int *)(iVar5 + 0x2c4) + 0x60) = 0;
  for (iVar6 = 0; iVar6 < *(int *)(iVar5 + 0x234); iVar6 = iVar6 + 1) {
    FUN_800406e8(*(undefined4 *)(iVar5 + 0x2d0),*(undefined4 *)(iVar5 + 0x2a0),iVar6,0);
  }
  uVar4 = 0;
  bVar2 = true;
  if ((*(int *)(iVar5 + 0x2dc) != 2) && (*(int *)(iVar5 + 0x2dc) != 3)) {
    bVar2 = false;
  }
  if (!bVar2) {
    dVar13 = (double)*(float *)(unaff_r2 + -0x7d0c);
    iVar10 = iVar5 + 0x238;
    for (iVar6 = 0; iVar6 < *(int *)(iVar5 + 0x234); iVar6 = iVar6 + 1) {
      iVar11 = FUN_800406e8(*(undefined4 *)(iVar5 + 0x2d0),*(undefined4 *)(iVar5 + 0x2a0),iVar6,
                            auStack_68);
      if (iVar11 != 0) {
        if ((-1 < *(int *)(iVar10 + 4)) && (*(int *)(iVar10 + 4) < 6)) {
          local_4c = (float)((double)local_4c + dVar13);
        }
        FUN_8003fb90(*(undefined4 *)(iVar5 + 0x2c4),iVar6,auStack_68);
        FUN_8003e928(*(undefined4 *)(iVar5 + 0x2c4),iVar6,iVar12);
        if ((uVar4 & 0x80000000) != 0) {
          FUN_8003efc8(*(undefined4 *)(iVar5 + 0x2c4),iVar6,&local_198);
          FUN_80040f98(*(undefined4 *)(iVar5 + 0x2c0),7,auStack_108,&local_198);
          FUN_8003c2fc(*(undefined4 *)(iVar5 + 700));
          FUN_800d9a74(iVar6,auStack_138,auStack_108);
          FUN_8003ea00(*(undefined4 *)(iVar5 + 0x2c4),iVar6,auStack_108,0,0);
          uVar4 = uVar4 | 1 << iVar6;
          FUN_800d9c34(iVar6);
        }
        bVar2 = true;
        if ((*(int *)(iVar5 + 0x2dc) != 2) && (*(int *)(iVar5 + 0x2dc) != 3)) {
          bVar2 = false;
        }
        if (((!bVar2) && (iVar11 = FUN_8003e538(*(undefined4 *)(iVar5 + 0x2c4),iVar6), iVar11 != 0))
           && (iVar7 = FUN_8003e5f0(*(undefined4 *)(iVar5 + 0x2c4),iVar6), iVar7 != 0)) {
          FUN_800366a8(*(undefined4 *)(iVar5 + 0x2c8),iVar11,iVar7 + 0x14,auStack_98,0,0,1,iVar7,0);
        }
      }
      iVar10 = iVar10 + 0x14;
    }
  }
  FUN_8003c278(*(undefined4 *)(iVar5 + 700));
  uVar3 = FUN_80040f24(*(undefined4 *)(iVar5 + 0x2c0),0);
  *(undefined4 *)(*(int *)(iVar5 + 0x2c4) + 0x60) = uVar3;
  FUN_8003fc08(*(undefined4 *)(iVar5 + 0x2c4),iVar12);
  local_1b0 = DAT_801c9370;
  local_1ac = DAT_801c9374;
  local_1a8 = DAT_801c9378;
  local_1bc = 0;
  local_1b8 = 0;
  local_1b4 = 0;
  local_1c8 = DAT_801c937c;
  local_1c4 = DAT_801c9380;
  local_1c0 = DAT_801c9384;
  FUN_80124ab0(auStack_168,&local_1c8,&local_1b0,&local_1bc);
  FUN_800365bc(*(undefined4 *)(iVar5 + 0x2c8),auStack_168,&local_1c8);
  for (iVar6 = 0; iVar6 < *(int *)(iVar5 + 0x234); iVar6 = iVar6 + 1) {
    iVar10 = FUN_800406e8(*(undefined4 *)(iVar5 + 0x2d0),*(undefined4 *)(iVar5 + 0x2a0),iVar6,
                          auStack_68);
    if (iVar10 != 0) {
      FUN_8003ebbc(*(undefined4 *)(iVar5 + 0x2c4),iVar6,auStack_168,&local_1c8);
    }
  }
  for (iVar6 = 0; iVar6 < *(int *)(iVar5 + 0x234); iVar6 = iVar6 + 1) {
    if ((uVar4 & 1 << iVar6) != 0) {
      FUN_800d98b0(iVar6,1);
    }
  }
  FUN_80036f40(*(undefined4 *)(iVar5 + 0x2c8),auStack_98,&local_1a4);
  for (iVar6 = 0; iVar6 < *(int *)(iVar5 + 0x234); iVar6 = iVar6 + 1) {
    if ((uVar4 & 1 << iVar6) != 0) {
      FUN_800d98b0(iVar6,0);
    }
  }
  for (iVar6 = 0; iVar6 < *(int *)(iVar5 + 0xd8); iVar6 = iVar6 + 1) {
    FUN_800a73e0(iVar5 + 0x2dc,iVar6,auStack_98,0,0);
  }
  FUN_8005c210(*(undefined4 *)(iVar5 + 0x2d4),iVar5 + 0x80,*(undefined4 *)(iVar5 + 0x74),
               *(undefined4 *)(iVar5 + 0x294));
  FUN_8005c6e4(*(undefined4 *)(iVar5 + 0x2d4),iVar12);
  FUN_800a75c8(iVar5 + 0x2dc,iVar12,auStack_98,auStack_d8,&local_1a4);
  bVar2 = true;
  if ((*(int *)(iVar5 + 0x2dc) != 2) && (*(int *)(iVar5 + 0x2dc) != 3)) {
    bVar2 = false;
  }
  if (bVar2) {
    FUN_800a7644(iVar5 + 0x2dc,auStack_98);
  }
  FUN_800363c4(*(undefined4 *)(iVar5 + 0x2c8),iVar12,auStack_98,auStack_d8,&local_1a4);
  bVar2 = true;
  if ((*(int *)(iVar5 + 0x2dc) != 2) && (*(int *)(iVar5 + 0x2dc) != 3)) {
    bVar2 = false;
  }
  if (!bVar2) {
    FUN_80037484(*(undefined4 *)(iVar5 + 0x2c8),auStack_98);
  }
  bVar2 = true;
  if ((*(int *)(iVar5 + 0x2dc) != 2) && (*(int *)(iVar5 + 0x2dc) != 3)) {
    bVar2 = false;
  }
  if (!bVar2) {
    FUN_80004104(&local_188,0,0x20);
    piVar8 = &local_188;
    for (iVar12 = 0; iVar6 = *(int *)(iVar5 + 0x234), iVar12 < iVar6; iVar12 = iVar12 + 1) {
      iVar6 = FUN_800406e8(*(undefined4 *)(iVar5 + 0x2d0),*(undefined4 *)(iVar5 + 0x2a0),iVar12,
                           auStack_68);
      if (iVar6 == 0) {
        *piVar8 = -1;
      }
      else {
        dVar13 = (double)FUN_8003fe60(*(undefined4 *)(iVar5 + 0x2c4),iVar12,auStack_98);
        piVar8[1] = (int)(float)dVar13;
        *piVar8 = iVar12;
      }
      piVar8 = piVar8 + 2;
    }
    piVar8 = &local_188;
    for (iVar12 = 0; iVar12 < iVar6 + -1; iVar12 = iVar12 + 1) {
      iVar10 = iVar12 + 1;
      piVar9 = &local_188 + iVar10 * 2;
      for (; iVar6 = *(int *)(iVar5 + 0x234), iVar10 < iVar6; iVar10 = iVar10 + 1) {
        if ((*piVar8 == -1) || ((float)piVar8[1] < (float)piVar9[1])) {
          local_1e8 = *piVar8;
          local_1e4 = piVar8[1];
          *piVar8 = *piVar9;
          piVar8[1] = piVar9[1];
          *piVar9 = local_1e8;
          piVar9[1] = local_1e4;
        }
        piVar9 = piVar9 + 2;
      }
      piVar8 = piVar8 + 2;
    }
    piVar8 = &local_188;
    for (iVar12 = 0; iVar12 < *(int *)(iVar5 + 0x234); iVar12 = iVar12 + 1) {
      if (*piVar8 != -1) {
        FUN_8003eaf8(*(undefined4 *)(iVar5 + 0x2c4),*piVar8,auStack_98,&local_1a4,1);
        FUN_8003eaf8(*(undefined4 *)(iVar5 + 0x2c4),*piVar8,auStack_98,&local_1a4,2);
      }
      piVar8 = piVar8 + 2;
    }
  }
  FUN_8005c7b4(*(undefined4 *)(iVar5 + 0x2d4));
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-((byte)(unaff_GQR0 >> 0x18) & 0x3f));
  }
  FUN_800ac92c();
  return;
}



// ===== FUN_800307f0 =====

/* WARNING: Removing unreachable block (ram,0x80030d34) */
/* WARNING: Removing unreachable block (ram,0x80030d2c) */

void FUN_800307f0(void)

{
  byte bVar1;
  bool bVar2;
  uint uVar3;
  int unaff_r2;
  int iVar4;
  undefined4 uVar5;
  int iVar6;
  int iVar7;
  short *psVar8;
  int unaff_r13;
  short *psVar9;
  short *psVar10;
  int iVar11;
  int *piVar12;
  short *psVar13;
  uint unaff_GQR0;
  double dVar14;
  double dVar15;
  undefined8 uVar16;
  
  bVar1 = (byte)(unaff_GQR0 >> 8);
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf(bVar1 & 0x3f);
  }
  if ((unaff_GQR0 & 0x3f00) != 0) {
    ldexpf(bVar1 & 0x3f);
  }
  uVar16 = FUN_800ac8d4();
  iVar4 = (int)((ulonglong)uVar16 >> 0x20);
  psVar9 = (short *)0x0;
  if (*(int *)(iVar4 + 0x28c) == 0) goto LAB_80030d2c;
  iVar11 = *(int *)(iVar4 + 0x290);
  dVar14 = (double)FUN_8001cb30(*(undefined4 *)(unaff_r13 + -0x7348));
  uVar5 = FUN_800ac7c4((double)(float)((double)*(float *)(unaff_r2 + -0x7d34) * dVar14));
  *(undefined4 *)(iVar4 + 0x294) = uVar5;
  iVar6 = *(int *)(*(int *)(unaff_r13 + -0x7348) + 0x400774) >> 0xc;
  *(int *)(iVar4 + 0x298) = iVar6;
  uVar3 = *(uint *)(*(int *)(unaff_r13 + -0x7348) + 0x400774) & 0xfff;
  *(uint *)(iVar4 + 0x29c) = uVar3;
  psVar10 = psVar9;
  if ((((int)uVar16 == 0) && (iVar11 != -1)) && (iVar7 = *(int *)(unaff_r13 + -0x7328), iVar7 != 0))
  {
    psVar8 = (short *)(iVar7 + iVar11 * 0x24);
    if (((psVar8[1] != -1) && (*psVar8 <= iVar6)) && ((int)psVar8[1] <= (int)uVar3)) {
      for (; (psVar10 = psVar9, psVar8[1] != -1 && (psVar10 = psVar8, iVar6 != *psVar8));
          psVar8 = psVar8 + 0x12) {
      }
    }
    if (psVar10 == (short *)0x0) {
      if (psVar8[1] == -1) {
        *(undefined4 *)(iVar4 + 0x290) = 0xffffffff;
      }
    }
    else {
      *(uint *)(iVar4 + 0x290) = (uint)((int)psVar10 - iVar7) / 0x24 + 1;
    }
  }
  if (psVar10 == (short *)0x0) goto LAB_80030d2c;
  iVar6 = *(int *)(iVar4 + 0x230);
  if (iVar6 == 4) {
LAB_8003094c:
    psVar9 = psVar10 + 6;
  }
  else {
    if (iVar6 < 4) {
      if ((iVar6 == 0) || (iVar6 < 0)) goto LAB_8003094c;
    }
    else if (5 < iVar6) goto LAB_8003094c;
    psVar9 = psVar10 + 8;
  }
  if ((*psVar9 != -1) && (*(int *)(*(int *)(iVar4 + 700) + 0x60) != 4)) {
    FUN_8003bec8((double)((float)((double)CONCAT44(0x43300000,(int)psVar9[1] ^ 0x80000000) -
                                 *(double *)(unaff_r2 + -0x7d28)) / *(float *)(unaff_r2 + -0x7d2c)),
                 *(int *)(iVar4 + 700),(int)*psVar9,0);
  }
  FUN_80030718(iVar4,psVar10);
  iVar6 = 0;
  piVar12 = (int *)(iVar4 + 0x2a4);
  do {
    psVar9 = (short *)0x0;
    psVar8 = (short *)0x0;
    if (*piVar12 != 0) {
      if (iVar6 == 1) {
        psVar9 = psVar10 + 4;
        psVar8 = psVar10 + 0x10;
      }
      else if ((iVar6 < 1) && (-1 < iVar6)) {
        psVar9 = psVar10 + 2;
        psVar8 = psVar10 + 0xe;
      }
      if (*psVar9 != -1) {
        if ((*(int *)(iVar4 + 0x2dc) == 3) &&
           (FUN_800a7678(iVar4 + 0x2dc), *(int *)(iVar4 + 0x2d8) != 0)) {
          FUN_80041388();
        }
        uVar5 = 1;
        if ((*(int *)(iVar4 + 0x2dc) != 2) && (*(int *)(iVar4 + 0x2dc) != 3)) {
          uVar5 = 0;
        }
        uVar3 = countLeadingZeros(uVar5);
        FUN_8003f3a4(*(undefined4 *)(iVar4 + 0x2c4),iVar6,uVar3 >> 5);
      }
      bVar2 = true;
      if ((*(int *)(iVar4 + 0x2dc) != 2) && (*(int *)(iVar4 + 0x2dc) != 3)) {
        bVar2 = false;
      }
      if (!bVar2) {
        if (*psVar8 != -1) {
          FUN_8003f550(*(undefined4 *)(iVar4 + 0x2c4),iVar6);
        }
        if (psVar8[1] != -1) {
          FUN_8003f644(*(undefined4 *)(iVar4 + 0x2c4),iVar6);
        }
      }
    }
    iVar6 = iVar6 + 1;
    piVar12 = piVar12 + 3;
  } while (iVar6 < 2);
  dVar14 = *(double *)(unaff_r2 + -0x7d28);
  dVar15 = (double)*(float *)(unaff_r2 + -0x7d2c);
  for (psVar9 = psVar10; psVar9[1] != -1; psVar9 = psVar9 + 0x12) {
    bVar2 = false;
    iVar6 = 0;
    piVar12 = (int *)(iVar4 + 0x2a4);
    do {
      psVar8 = (short *)0x0;
      psVar13 = (short *)0x0;
      if (iVar6 == 1) {
        psVar8 = psVar9 + 4;
        psVar13 = psVar9 + 0x10;
      }
      else if ((iVar6 < 1) && (-1 < iVar6)) {
        psVar8 = psVar9 + 2;
        psVar13 = psVar9 + 0xe;
      }
      if (*piVar12 == 0) {
        if (*psVar8 != -1) {
          FUN_8003f0c4((double)((float)((double)(float)((double)CONCAT44(0x43300000,
                                                                         (int)psVar8[1] ^ 0x80000000
                                                                        ) - dVar14) / dVar15) /
                               *(float *)(&DAT_801c8418 + *(int *)(iVar4 + 0xc0) * 4)),
                       *(undefined4 *)(iVar4 + 0x2c4),iVar6);
          if ((*(int *)(iVar4 + 0x2dc) == 3) &&
             (FUN_800a7678(iVar4 + 0x2dc), *(int *)(iVar4 + 0x2d8) != 0)) {
            FUN_80041388();
          }
          uVar5 = 1;
          if ((*(int *)(iVar4 + 0x2dc) != 2) && (*(int *)(iVar4 + 0x2dc) != 3)) {
            uVar5 = 0;
          }
          uVar3 = countLeadingZeros(uVar5);
          FUN_8003f3a4(*(undefined4 *)(iVar4 + 0x2c4),iVar6,uVar3 >> 5);
          *piVar12 = 1;
        }
        bVar2 = true;
        if ((*(int *)(iVar4 + 0x2dc) != 2) && (*(int *)(iVar4 + 0x2dc) != 3)) {
          bVar2 = false;
        }
        if (!bVar2) {
          if (*psVar13 != -1) {
            FUN_8003f550(*(undefined4 *)(iVar4 + 0x2c4),iVar6);
          }
          if (psVar13[1] != -1) {
            FUN_8003f644(*(undefined4 *)(iVar4 + 0x2c4),iVar6);
          }
        }
        bVar2 = true;
      }
      else if (piVar12[1] * 0x1000 + piVar12[2] < *psVar9 * 0x1000 + (int)psVar9[1]) {
        if (*psVar8 == -1) {
          bVar2 = true;
        }
        else {
          iVar11 = FUN_8003f0c4((double)((float)((double)(float)((double)CONCAT44(0x43300000,
                                                                                  (int)psVar8[1] ^
                                                                                  0x80000000) -
                                                                dVar14) / dVar15) /
                                        *(float *)(&DAT_801c8418 + *(int *)(iVar4 + 0xc0) * 4)),
                                *(undefined4 *)(iVar4 + 0x2c4),iVar6);
          if (iVar11 != 0) {
            bVar2 = true;
            piVar12[1] = (int)*psVar9;
            piVar12[2] = (int)psVar9[1];
          }
        }
      }
      else {
        bVar2 = true;
      }
      iVar6 = iVar6 + 1;
      piVar12 = piVar12 + 3;
    } while (iVar6 < 2);
    if (!bVar2) break;
  }
  iVar6 = *(int *)(iVar4 + 0x2d8);
  if (iVar6 != 0) {
    iVar11 = 1;
    if ((*(int *)(iVar4 + 0x2dc) != 2) && (*(int *)(iVar4 + 0x2dc) != 3)) {
      iVar11 = 0;
    }
    (**(code **)(*(int *)(iVar6 + 0x270) + 0x1c))
              (iVar6,*(undefined4 *)(psVar10 + 10),(uint)-iVar11 >> 0x1f);
  }
LAB_80030d2c:
  bVar1 = (byte)(unaff_GQR0 >> 0x18);
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-(bVar1 & 0x3f));
  }
  if ((unaff_GQR0 & 0x3f000000) != 0) {
    ldexpf(-(bVar1 & 0x3f));
  }
  FUN_800ac920();
  return;
}



// ===== FUN_80028130 =====

int FUN_80028130(int param_1,int param_2,int param_3,int param_4,uint param_5,int param_6)

{
  int iVar1;
  int unaff_r2;
  uint uVar2;
  int iVar3;
  int iVar4;
  uint uVar5;
  int iVar6;
  int iVar7;
  undefined8 local_18;
  undefined8 local_10;
  
  iVar1 = (int)(param_5 << 6) / 100 + ((int)(param_5 << 6 | param_5 >> 0x1a) >> 0x1f);
  iVar1 = iVar1 - (iVar1 >> 0x1f);
  if (param_6 != 0) {
    if (param_6 < 0) {
      if (-2 < param_6) {
        iVar4 = 10;
        goto LAB_800281dc;
      }
    }
    else if (param_6 < 2) {
      iVar4 = 0;
      goto LAB_800281dc;
    }
  }
  if (((*(uint *)(param_2 + 0x10) & 1) == 0) || ('\0' < *(char *)(param_2 + 0x15))) {
    if ((*(uint *)(param_1 + 0x24) & 0x7e0000) == 0) {
      iVar4 = *(int *)(param_2 + 0x28) - *(int *)(param_2 + 0x34);
    }
    else {
      iVar4 = 0;
    }
    uVar2 = iVar4 + -1 >> 0x1f;
    iVar4 = (uVar2 ^ iVar4 + -1 >> 1) - uVar2;
  }
  else {
    iVar4 = 0xf;
  }
LAB_800281dc:
  if (iVar4 < 10) {
    iVar3 = *(int *)(param_1 + 0xa09e4);
    if (iVar3 < 0x401) {
      iVar7 = (iVar3 * (iVar3 + 1)) / (*(int *)(param_1 + 0xa09e8) + 1);
    }
    else {
      iVar7 = 0x1e;
    }
    if (0x14 < iVar7) {
      iVar7 = (iVar7 + -0x14) / 10 + (iVar7 + -0x14 >> 0x1f);
      iVar7 = (iVar7 - (iVar7 >> 0x1f)) + 0x14;
    }
    if (0x1e < iVar7) {
      iVar7 = 0x1e;
    }
    iVar4 = (0x15 - iVar4 >> 1) * (iVar7 * iVar7 + 400);
    iVar4 = iVar4 / 500 + (iVar4 >> 0x1f);
    iVar1 = (((iVar4 - (iVar4 >> 0x1f)) * 2000) / (*(int *)(param_1 + 0xa0970) + 1000)) *
            (iVar1 * -2 + 0xc0);
    iVar1 = iVar1 / 0xc0 + (iVar1 >> 0x1f);
    uVar5 = iVar1 - (iVar1 >> 0x1f);
    if ((*(uint *)(param_1 + 0xa0964) & 1) != 0) {
      uVar5 = iVar1 / 2;
    }
    if ((*(uint *)(param_1 + 0x24) & 0x4000) != 0) {
      local_18 = (double)CONCAT44(0x43300000,uVar5 ^ 0x80000000);
      uVar5 = (uint)((float)(local_18 - *(double *)(unaff_r2 + -0x7de0)) *
                    *(float *)(unaff_r2 + -0x7dc8));
    }
    if ((int)uVar5 < 2) {
      uVar5 = 2;
    }
    if ((*(int *)(param_1 + 0xa0970) == 0) && (iVar3 < 3)) {
      uVar5 = 0;
    }
  }
  else {
    iVar7 = *(int *)(param_1 + 0xa09e8);
    iVar3 = iVar7;
    if (0x1e < iVar7) {
      iVar3 = 0x1e;
    }
    iVar6 = *(int *)(param_1 + 0xa0970);
    iVar3 = (iVar4 + -1) * 4 * (iVar3 * iVar3 + 700);
    iVar4 = *(int *)(param_1 + 0xa09ec);
    uVar2 = *(uint *)(param_1 + 0x24) & 0x4000;
    iVar3 = iVar3 / 700 + (iVar3 >> 0x1f);
    iVar3 = (iVar3 - (iVar3 >> 0x1f)) * (iVar6 * 2 + 1000);
    iVar3 = iVar3 / 3000 + (iVar3 >> 0x1f);
    uVar5 = (iVar3 - (iVar3 >> 0x1f)) * (iVar1 * 3 + 0x40);
    if (uVar2 == 0) {
      if ((0xc - iVar7 < iVar4) && (4 < iVar4)) {
        iVar4 = 0;
      }
      if (*(int *)(param_1 + 0xa09f4) + 0xc00 < param_4) {
        iVar4 = 0;
      }
    }
    if (8 < iVar4) {
      iVar4 = 8;
    }
    uVar5 = (int)((((int)uVar5 >> 6) + (uint)((int)uVar5 < 0 && (uVar5 & 0x3f) != 0)) * 3) /
            (iVar4 + 2);
    if ((uVar2 != 0) && (uVar5 != 0)) {
      local_10 = (double)CONCAT44(0x43300000,uVar5 ^ 0x80000000);
      uVar5 = (uint)((float)(local_10 - *(double *)(unaff_r2 + -0x7de0)) *
                    *(float *)(unaff_r2 + -0x7dc4));
    }
    if ((iVar6 < (int)uVar5) && (0x7d < iVar6)) {
      uVar5 = iVar6 - 0x3e;
    }
    uVar5 = -uVar5;
  }
  uVar2 = (param_3 - 6000U) * 0x40;
  iVar1 = (int)uVar2 / 6000 + ((int)(uVar2 | param_3 - 6000U >> 0x1a) >> 0x1f);
  iVar1 = iVar1 - (iVar1 >> 0x1f);
  iVar1 = 0x1000 - iVar1 * iVar1;
  if (iVar1 < 0) {
    iVar1 = 0;
  }
  uVar5 = uVar5 * (iVar1 + 0x1000);
  return ((int)uVar5 >> 0xd) + (uint)((int)uVar5 < 0 && (uVar5 & 0x1fff) != 0);
}



// ===== FUN_800284a8 =====

void FUN_800284a8(int param_1,int param_2)

{
  int unaff_r2;
  uint uVar1;
  
  if (0 < *(int *)(param_1 + 0xa0970)) {
    *(int *)(param_1 + 0xa0970) = *(int *)(param_1 + 0xa0970) + param_2;
  }
  if (*(int *)(param_1 + 0xa0970) < 1) {
    uVar1 = *(uint *)(param_1 + 0xa0964);
    *(undefined4 *)(param_1 + 0xa0970) = 0;
    *(uint *)(param_1 + 0xa0964) = uVar1 | 1;
    if ((*(uint *)(param_1 + 0x24) & 0x2000) != 0) {
      *(uint *)(param_1 + 0xa0964) = uVar1 | 0x41;
    }
    FUN_80028010(param_1);
  }
  if (*(float *)(unaff_r2 + -0x7dc0) <
      (float)((double)CONCAT44(0x43300000,*(uint *)(param_1 + 0xa0970) ^ 0x80000000) -
             *(double *)(unaff_r2 + -0x7de0))) {
    *(uint *)(param_1 + 0xa0964) = *(uint *)(param_1 + 0xa0964) & 0xfffffffd;
  }
  else {
    *(uint *)(param_1 + 0xa0964) = *(uint *)(param_1 + 0xa0964) | 2;
  }
  if (((*(uint *)(param_1 + 0x24) & 0x8000) == 0) && ((*(uint *)(param_1 + 0xa0964) & 0x20) == 0)) {
    uVar1 = *(uint *)(param_1 + 0xa0970) ^ 0x80000000;
    if ((*(float *)(unaff_r2 + -0x7dbc) <
         (float)((double)CONCAT44(0x43300000,uVar1) - *(double *)(unaff_r2 + -0x7de0))) ||
       (0xc < *(int *)(param_1 + 0xa09e4))) {
      *(uint *)(param_1 + 0xa0a84) = *(uint *)(param_1 + 0xa0a84) | 2;
    }
    else if ((float)((double)CONCAT44(0x43300000,uVar1) - *(double *)(unaff_r2 + -0x7de0)) <
             *(float *)(unaff_r2 + -0x7dc0)) {
      *(uint *)(param_1 + 0xa0a84) = *(uint *)(param_1 + 0xa0a84) | 4;
    }
  }
  if (1000 < *(int *)(param_1 + 0xa0970)) {
    *(undefined4 *)(param_1 + 0xa0970) = 1000;
  }
  return;
}

