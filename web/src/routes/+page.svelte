<script lang="ts">
	import AddMirror from './AddMirror.svelte';
	import RemoveMirror from './RemoveMirror.svelte';

    let items = $state([]);
    let whipServer = $state();

    let modal;
    let ModalContent = $state();

    let states = {
        closeModal: () => modal.hidePopover(),
        refreshItems: async () => await fetchItems(),
        getItems: () => items
    };

    function openAddMirror() {
        ModalContent = (a) => AddMirror(a, { states });
        modal.showPopover();
    }

    function openRemoveMirror(mirror) {
        ModalContent = (a) => RemoveMirror(a, { mirror, states });
        modal.showPopover();
    }

    async function fetchItems() {
        const res = await fetch("/api/mirrors");
        items = await res.json();
    }

    $effect(async () => {
        await fetchItems();
        whipServer = window.location.origin + "/whip";
    });
</script>

<svelte:head>
    <title>utsuru</title>
</svelte:head>

<div bind:this={modal} class="m-auto bg-zinc-900 rounded-2xl backdrop:bg-zinc-600/40 backdrop:backdrop-blur" popover>
    {#if ModalContent}
        <ModalContent />
    {/if}
</div>
<div class="h-screen flex justify-center items-center">
    <div class="size-full md:max-w-192 md:max-h-108 overflow-hidden flex flex-col bg-zinc-900 rounded-2xl border border-zinc-800 text-sm text-zinc-500">
        <div class="px-3 py-1.5 flex space-x-1.5 bg-zinc-700 text-gray-100">
            <span class="flex-auto">utsuru</span>
            <a class="my-0.5 cursor-pointer" href="https://github.com/VincentVerdynanta/utsuru" target="_blank">
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 496 512">
                    <!-- Icon from Font Awesome Brands by Dave Gandy - https://creativecommons.org/licenses/by/4.0/ -->
                    <path fill="currentColor" d="M165.9 397.4c0 2-2.3 3.6-5.2 3.6c-3.3.3-5.6-1.3-5.6-3.6c0-2 2.3-3.6 5.2-3.6c3-.3 5.6 1.3 5.6 3.6m-31.1-4.5c-.7 2 1.3 4.3 4.3 4.9c2.6 1 5.6 0 6.2-2s-1.3-4.3-4.3-5.2c-2.6-.7-5.5.3-6.2 2.3m44.2-1.7c-2.9.7-4.9 2.6-4.6 4.9c.3 2 2.9 3.3 5.9 2.6c2.9-.7 4.9-2.6 4.6-4.6c-.3-1.9-3-3.2-5.9-2.9M244.8 8C106.1 8 0 113.3 0 252c0 110.9 69.8 205.8 169.5 239.2c12.8 2.3 17.3-5.6 17.3-12.1c0-6.2-.3-40.4-.3-61.4c0 0-70 15-84.7-29.8c0 0-11.4-29.1-27.8-36.6c0 0-22.9-15.7 1.6-15.4c0 0 24.9 2 38.6 25.8c21.9 38.6 58.6 27.5 72.9 20.9c2.3-16 8.8-27.1 16-33.7c-55.9-6.2-112.3-14.3-112.3-110.5c0-27.5 7.6-41.3 23.6-58.9c-2.6-6.5-11.1-33.3 2.6-67.9c20.9-6.5 69 27 69 27c20-5.6 41.5-8.5 62.8-8.5s42.8 2.9 62.8 8.5c0 0 48.1-33.6 69-27c13.7 34.7 5.2 61.4 2.6 67.9c16 17.7 25.8 31.5 25.8 58.9c0 96.5-58.9 104.2-114.8 110.5c9.2 7.9 17 22.9 17 46.4c0 33.7-.3 75.4-.3 83.6c0 6.5 4.6 14.4 17.3 12.1C428.2 457.8 496 362.9 496 252C496 113.3 383.5 8 244.8 8M97.2 352.9c-1.3 1-1 3.3.7 5.2c1.6 1.6 3.9 2.3 5.2 1c1.3-1 1-3.3-.7-5.2c-1.6-1.6-3.9-2.3-5.2-1m-10.8-8.1c-.7 1.3.3 2.9 2.3 3.9c1.6 1 3.6.7 4.3-.7c.7-1.3-.3-2.9-2.3-3.9c-2-.6-3.6-.3-4.3.7m32.4 35.6c-1.6 1.3-1 4.3 1.3 6.2c2.3 2.3 5.2 2.6 6.5 1c1.3-1.3.7-4.3-1.3-6.2c-2.2-2.3-5.2-2.6-6.5-1m-11.4-14.7c-1.6 1-1.6 3.6 0 5.9s4.3 3.3 5.6 2.3c1.6-1.3 1.6-3.9 0-6.2c-1.4-2.3-4-3.3-5.6-2"/>
                </svg>
            </a>
        </div>
        <div class="flex-auto relative">
            <div class="grid overflow-auto absolute inset-0 md:grid-cols-2 gap-4 p-4">
                <div class="px-4 py-2.5 flex flex-col bg-zinc-800 rounded-md border border-transparent hover:border-indigo-500 transition group">
                    <div class="mb-2 flex items-center group-hover:text-zinc-400 uppercase tracking-widest">
                        <span class="my-1.5 mr-2 underline underline-offset-4 decoration-zinc-400/20 group-hover:decoration-zinc-400/40 transition">Mirrors</span>
                        <button class="p-2 hover:bg-zinc-700 rounded-full transition cursor-pointer" onclick={openAddMirror}>
                            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24">
                                <!-- Icon from Lucide by Lucide Contributors - https://github.com/lucide-icons/lucide/blob/main/LICENSE -->
                                <path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14m-7-7v14"/>
                            </svg>
                        </button>
                        <button class="p-2 hover:bg-zinc-700 rounded-full transition cursor-pointer" onclick={fetchItems}>
                            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24">
                                <!-- Icon from Lucide by Lucide Contributors - https://github.com/lucide-icons/lucide/blob/main/LICENSE -->
                                <g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2"><path d="M21 12a9 9 0 0 0-9-9a9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5m-5 4a9 9 0 0 0 9 9a9.75 9.75 0 0 0 6.74-2.74L21 16"/><path d="M16 16h5v5"/></g>
                            </svg>
                        </button>
                    </div>
                    {#if !items.some((e) => e)}
                        <span class="text-zinc-400/40 group-hover:text-zinc-500 transition">There are no active mirrors yet.</span>
                    {/if}
                    {#each items as item, i}
                        {#if item}
                            <div class="nth-2:mt-2.5 last:mb-2.5 relative flex group/mirrors hover:text-zinc-300 hover:bg-zinc-700 rounded-md transition">
                                <div class="relative mx-2 w-1">
                                    <div class="absolute top-0 group-nth-2/mirrors:-top-2.5 right-0 bottom-0 group-last/mirrors:-bottom-2.5 left-0 bg-zinc-700 group-nth-2/mirrors:rounded-t-lg group-last/mirrors:rounded-b-lg"></div>
                                </div>
                                <div class="flex-auto px-1.5 py-0.5">
                                    <span>Discord Live {i + 1}</span>
                                </div>
                                <button class="my-1 mr-1 cursor-pointer" onclick={() => openRemoveMirror(i)}>
                                    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="-2 -2 28 28">
                                        <!-- Icon from Lucide by Lucide Contributors - https://github.com/lucide-icons/lucide/blob/main/LICENSE -->
                                        <path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 6h18m-2 0v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6m3 0V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2m-6 5v6m4-6v6"/>
                                    </svg>
                                </button>
                                <div class="flex absolute left-1.5 top-0 bottom-0 items-center">
                                    <div class="w-2 h-2 bg-zinc-500 group-hover/mirrors:bg-zinc-300 rounded-full transition"></div>
                                </div>
                            </div>
                        {/if}
                    {/each}
                </div>
                <div class="px-4 py-2.5 flex flex-col bg-zinc-800 rounded-md border border-transparent hover:border-indigo-500 transition group">
                    <div class="mb-2 flex items-center space-x-2 group-hover:text-zinc-400 uppercase tracking-widest">
                        <span class="my-1.5 underline underline-offset-4 decoration-zinc-400/20 group-hover:decoration-zinc-400/40 transition">WHIP</span>
                    </div>
                    <div class="flex flex-col space-y-2 group-hover:text-zinc-400 transition">
                        <span class="mb-3">Instructions for using utsuru with OBS:</span>
                        <div class="flex space-x-2">
                            <div class="w-8 h-8 flex justify-center items-center group-hover:bg-zinc-700 rounded-full border border-zinc-700 transition">
                                <span>1</span>
                            </div>
                            <div class="flex-auto flex flex-col justify-center *:my-1.5">
                                <span>Open <b>Settings</b> &gt; <b>Stream</b></span>
                            </div>
                        </div>
                        <div class="flex space-x-2">
                            <div class="w-8 h-8 flex justify-center items-center group-hover:bg-zinc-700 rounded-full border border-zinc-700 transition">
                                <span>2</span>
                            </div>
                            <div class="flex-auto flex flex-col justify-center *:my-1.5">
                                <span>Set the <b>Service</b> to <b>WHIP</b></span>
                            </div>
                        </div>
                        <div class="flex space-x-2">
                            <div class="w-8 h-8 flex justify-center items-center group-hover:bg-zinc-700 rounded-full border border-zinc-700 transition">
                                <span>3</span>
                            </div>
                            <div class="flex-auto flex flex-col justify-center *:my-1.5">
                                <span>Fill the <b>Destination</b> properties with:</span>
                                <div class="overflow-hidden flex flex-col group-hover:bg-zinc-700/40 rounded-md border border-zinc-700 transition">
                                    <div class="px-1.5 py-0.5 bg-zinc-500/20 group-hover:bg-zinc-700 text-xs transition">
                                        <span>Server</span>
                                    </div>
                                    <div class="h-8 relative">
                                        <div class="flex absolute inset-0 items-center overflow-auto">
                                            <input type="text" class="size-full px-3" value="{whipServer}" disabled>
                                        </div>
                                    </div>
                                </div>
                                <div class="overflow-hidden flex flex-col group-hover:bg-zinc-700/40 rounded-md border border-zinc-700 transition">
                                    <div class="px-1.5 py-0.5 bg-zinc-500/20 group-hover:bg-zinc-700 text-xs transition">
                                        <span>Bearer Token (WIP, any value works)</span>
                                    </div>
                                    <div class="h-8 relative">
                                        <div class="flex absolute inset-0 items-center overflow-auto">
                                            <input type="text" class="size-full px-3" value="utsuru" disabled>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </div>
</div>

<style lang="postcss">
	@reference "tailwindcss";

    :global(html) {
        background-color: theme(--color-black);
    }
</style>
