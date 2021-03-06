/* Copyright (C) 1995-1998 Eric Young (eay@cryptsoft.com)
 * All rights reserved.
 *
 * This package is an SSL implementation written
 * by Eric Young (eay@cryptsoft.com).
 * The implementation was written so as to conform with Netscapes SSL.
 *
 * This library is free for commercial and non-commercial use as long as
 * the following conditions are aheared to.  The following conditions
 * apply to all code found in this distribution, be it the RC4, RSA,
 * lhash, DES, etc., code; not just the SSL code.  The SSL documentation
 * included with this distribution is covered by the same copyright terms
 * except that the holder is Tim Hudson (tjh@cryptsoft.com).
 *
 * Copyright remains Eric Young's, and as such any Copyright notices in
 * the code are not to be removed.
 * If this package is used in a product, Eric Young should be given attribution
 * as the author of the parts of the library used.
 * This can be in the form of a textual message at program startup or
 * in documentation (online or textual) provided with the package.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *    "This product includes cryptographic software written by
 *     Eric Young (eay@cryptsoft.com)"
 *    The word 'cryptographic' can be left out if the rouines from the library
 *    being used are not cryptographic related :-).
 * 4. If you include any Windows specific code (or a derivative thereof) from
 *    the apps directory (application code) you must include an acknowledgement:
 *    "This product includes software written by Tim Hudson (tjh@cryptsoft.com)"
 *
 * THIS SOFTWARE IS PROVIDED BY ERIC YOUNG ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * The licence and distribution terms for any publically available version or
 * derivative of this code cannot be changed.  i.e. this code cannot simply be
 * copied and put under another distribution licence
 * [including the GNU Public Licence.] */

#include <openssl/bn.h>

#include <assert.h>

#include "internal.h"


#define mul_add(r, a, w, c)             \
  {                                     \
    BN_ULONG high, low, ret, tmp = (a); \
    ret = (r);                          \
    bn_umult_lohi(&low, &high, w, tmp); \
    ret += (c);                         \
    (c) = (ret < (c)) ? 1 : 0;          \
    (c) += high;                        \
    ret += low;                         \
    (c) += (ret < low) ? 1 : 0;         \
    (r) = ret;                          \
  }

#define mul(r, a, w, c)                \
  {                                    \
    BN_ULONG high, low, ret, ta = (a); \
    bn_umult_lohi(&low, &high, w, ta); \
    ret = low + (c);                   \
    (c) = high;                        \
    (c) += (ret < low) ? 1 : 0;        \
    (r) = ret;                         \
  }

#define sqr(r0, r1, a)               \
  {                                  \
    BN_ULONG tmp = (a);              \
    bn_umult_lohi(&r0, &r1, tmp, tmp); \
  }


BN_ULONG bn_mul_add_words(BN_ULONG *rp, const BN_ULONG *ap, int num,
                          BN_ULONG w) {
  BN_ULONG c1 = 0;

  assert(num >= 0);
  if (num <= 0) {
    return c1;
  }

  while (num & ~3) {
    mul_add(rp[0], ap[0], w, c1);
    mul_add(rp[1], ap[1], w, c1);
    mul_add(rp[2], ap[2], w, c1);
    mul_add(rp[3], ap[3], w, c1);
    ap += 4;
    rp += 4;
    num -= 4;
  }

  while (num) {
    mul_add(rp[0], ap[0], w, c1);
    ap++;
    rp++;
    num--;
  }

  return c1;
}

BN_ULONG bn_mul_words(BN_ULONG *rp, const BN_ULONG *ap, int num, BN_ULONG w) {
  BN_ULONG c1 = 0;

  assert(num >= 0);
  if (num <= 0) {
    return c1;
  }

  while (num & ~3) {
    mul(rp[0], ap[0], w, c1);
    mul(rp[1], ap[1], w, c1);
    mul(rp[2], ap[2], w, c1);
    mul(rp[3], ap[3], w, c1);
    ap += 4;
    rp += 4;
    num -= 4;
  }
  while (num) {
    mul(rp[0], ap[0], w, c1);
    ap++;
    rp++;
    num--;
  }
  return c1;
}

BN_ULONG bn_add_words(BN_ULONG *r, const BN_ULONG *a, const BN_ULONG *b,
                      int n) {
  BN_ULONG c, l, t;

  assert(n >= 0);
  if (n <= 0) {
    return (BN_ULONG)0;
  }

  c = 0;
  while (n & ~3) {
    t = a[0];
    t = (t + c) & BN_MASK2;
    c = (t < c);
    l = (t + b[0]) & BN_MASK2;
    c += (l < t);
    r[0] = l;
    t = a[1];
    t = (t + c) & BN_MASK2;
    c = (t < c);
    l = (t + b[1]) & BN_MASK2;
    c += (l < t);
    r[1] = l;
    t = a[2];
    t = (t + c) & BN_MASK2;
    c = (t < c);
    l = (t + b[2]) & BN_MASK2;
    c += (l < t);
    r[2] = l;
    t = a[3];
    t = (t + c) & BN_MASK2;
    c = (t < c);
    l = (t + b[3]) & BN_MASK2;
    c += (l < t);
    r[3] = l;
    a += 4;
    b += 4;
    r += 4;
    n -= 4;
  }
  while (n) {
    t = a[0];
    t = (t + c) & BN_MASK2;
    c = (t < c);
    l = (t + b[0]) & BN_MASK2;
    c += (l < t);
    r[0] = l;
    a++;
    b++;
    r++;
    n--;
  }
  return (BN_ULONG)c;
}

BN_ULONG bn_sub_words(BN_ULONG *r, const BN_ULONG *a, const BN_ULONG *b,
                      int n) {
  BN_ULONG t1, t2;
  int c = 0;

  assert(n >= 0);
  if (n <= 0) {
    return (BN_ULONG)0;
  }

  while (n & ~3) {
    t1 = a[0];
    t2 = b[0];
    r[0] = (t1 - t2 - c) & BN_MASK2;
    if (t1 != t2) {
      c = (t1 < t2);
    }
    t1 = a[1];
    t2 = b[1];
    r[1] = (t1 - t2 - c) & BN_MASK2;
    if (t1 != t2) {
      c = (t1 < t2);
    }
    t1 = a[2];
    t2 = b[2];
    r[2] = (t1 - t2 - c) & BN_MASK2;
    if (t1 != t2) {
      c = (t1 < t2);
    }
    t1 = a[3];
    t2 = b[3];
    r[3] = (t1 - t2 - c) & BN_MASK2;
    if (t1 != t2) {
      c = (t1 < t2);
    }
    a += 4;
    b += 4;
    r += 4;
    n -= 4;
  }
  while (n) {
    t1 = a[0];
    t2 = b[0];
    r[0] = (t1 - t2 - c) & BN_MASK2;
    if (t1 != t2) {
      c = (t1 < t2);
    }
    a++;
    b++;
    r++;
    n--;
  }
  return c;
}
