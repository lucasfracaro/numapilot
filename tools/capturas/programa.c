#include <unistd.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
static double f; void* w(void*a){ long n=(long)a; for(;;){ for(long i=0;i<n*1000000;i++) f+=i*0.5; usleep(1000);} }
int main(int c,char**v){ int t=c>1?atoi(v[1]):1; long n=c>2?atol(v[2]):5; long mb=c>3?atol(v[3]):0; if(mb){char*p=malloc(mb<<20); memset(p,1,mb<<20);} for(int i=0;i<t;i++){pthread_t p;pthread_create(&p,0,w,(void*)n);} for(;;) sleep(100); }
