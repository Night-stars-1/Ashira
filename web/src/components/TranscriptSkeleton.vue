<template>
  <!-- Mirrors Transcript's column: a user bubble on the right, a reply below. -->
  <div role="status" aria-label="正在加载" class="mx-auto flex w-full max-w-3xl flex-col gap-4 px-3 py-4 sm:px-6 sm:py-6">
    <template v-for="turn in 2" :key="turn">
      <div class="ml-auto h-9 w-2/5 rounded-2xl bg-muted motion-safe:animate-pulse" />
      <div class="flex flex-col gap-2">
        <div class="h-3.5 w-11/12 rounded bg-muted motion-safe:animate-pulse" />
        <div class="h-3.5 w-4/5 rounded bg-muted motion-safe:animate-pulse" />
        <div class="h-3.5 w-3/5 rounded bg-muted motion-safe:animate-pulse" />
      </div>
    </template>
  </div>
</template>
